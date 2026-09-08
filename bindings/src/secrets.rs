//! The built-in secret acceptors of docs/ROADMAP.md 7.3 and 5.13:
//! `requester()` and `hold(until: :wane | DURATION)`. Everything else is a
//! hook.
//!
//! Neither writes anything to the store or to disk. `requester()` hands
//! the value to the client attached to the verb that produced it, and
//! accepts only while one is attached; `hold()` keeps it in the daemon's
//! memory until its bound, gives it up once to `rue reveal`, and is
//! emptied by a restart.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rue_core::model::Instant;
use rue_engine::executor::ExecError;
use rue_engine::secrets::{Acceptor, Mailbox};

/// `requester()`: the client attached to the verb that produced the
/// secret. It accepts only while one is attached, and what it takes is
/// drained into that verb's reply by the control handler.
#[derive(Debug, Clone, Default)]
pub struct Requester(pub Mailbox);

impl Requester {
    pub fn new(m: Mailbox) -> Requester {
        Requester(m)
    }
}

impl Acceptor for Requester {
    fn name(&self) -> &str {
        "requester"
    }

    fn deliver(
        &mut self,
        instance: &str,
        label: &str,
        value: &str,
        _now: Instant,
        _until: Option<Instant>,
    ) -> Result<bool, ExecError> {
        self.0.with(|a| {
            if !a.attached {
                return Ok(false);
            }
            a.pending
                .entry(instance.to_string())
                .or_default()
                .push((label.to_string(), value.to_string()));
            Ok(true)
        })
    }

    fn drop_for(&mut self, instance: &str) -> Vec<String> {
        self.0.with(|a| {
            a.pending
                .remove(instance)
                .unwrap_or_default()
                .into_iter()
                .map(|(l, _)| l)
                .collect()
        })
    }

    fn drop_all(&mut self) -> Vec<(String, String)> {
        self.0.with(|a| {
            let mut v = Vec::new();
            for (i, entries) in std::mem::take(&mut a.pending) {
                for (l, _) in entries {
                    v.push((i.clone(), l));
                }
            }
            v
        })
    }
}

/// One secret in memory, with the instant it stops being available.
#[derive(Debug, Clone)]
struct Kept {
    label: String,
    value: String,
    until: Option<Instant>,
}

/// `hold(until: :wane | DURATION)`: the daemon's memory, and nowhere else.
#[derive(Debug, Clone, Default)]
pub struct Hold {
    /// A declared duration; `None` is `until: :wane`, whose bound the
    /// engine computes per instance (R0104 when it cannot).
    pub duration: Option<rue_core::model::Duration>,
    kept: Arc<Mutex<BTreeMap<String, Kept>>>,
}

impl Hold {
    pub fn new(duration: Option<rue_core::model::Duration>) -> Hold {
        Hold {
            duration,
            kept: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
    fn with<T>(&self, f: impl FnOnce(&mut BTreeMap<String, Kept>) -> T) -> T {
        f(&mut self.kept.lock().unwrap_or_else(|e| e.into_inner()))
    }
    /// Whether anything is held for an instance, for a test or `rue
    /// doctor`; never the value.
    pub fn holds(&self, instance: &str) -> bool {
        self.with(|k| k.contains_key(instance))
    }
}

impl Acceptor for Hold {
    fn name(&self) -> &str {
        "hold"
    }

    fn deliver(
        &mut self,
        instance: &str,
        label: &str,
        value: &str,
        now: Instant,
        until: Option<Instant>,
    ) -> Result<bool, ExecError> {
        // `hold(until: 10m)` and the plan's wane are both bounds; the
        // earlier one is the bound, because neither is allowed to extend
        // the other.
        let own = self.duration.map(|d| now.plus(d));
        let bound = match (own, until) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, None) => a,
            (None, b) => b,
        };
        self.with(|k| {
            k.insert(
                instance.to_string(),
                Kept {
                    label: label.to_string(),
                    value: value.to_string(),
                    until: bound,
                },
            );
        });
        Ok(true)
    }

    fn take(&mut self, instance: &str) -> Option<(String, String)> {
        self.with(|k| k.remove(instance).map(|v| (v.label, v.value)))
    }

    fn drop_for(&mut self, instance: &str) -> Vec<String> {
        self.with(|k| {
            k.remove(instance)
                .map(|v| vec![v.label])
                .unwrap_or_default()
        })
    }

    fn expire(&mut self, now: Instant) -> Vec<(String, String)> {
        self.with(|k| {
            let gone: Vec<String> = k
                .iter()
                .filter(|(_, v)| v.until.is_some_and(|u| now.unix_s >= u.unix_s))
                .map(|(i, _)| i.clone())
                .collect();
            gone.into_iter()
                .filter_map(|i| k.remove(&i).map(|v| (i, v.label)))
                .collect()
        })
    }

    fn drop_all(&mut self) -> Vec<(String, String)> {
        self.with(|k| {
            std::mem::take(k)
                .into_iter()
                .map(|(i, v)| (i, v.label))
                .collect()
        })
    }
}
