//! The built-in secret bindings of docs/ROADMAP.md 7.3 and 5.13: the
//! acceptors `requester()` and `hold(until: :wane | DURATION)` that a
//! value is delivered to, and the source `file()` a `secret(:ref)` is
//! resolved from. Everything else is a hook.
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
use rue_engine::secrets::{Acceptor, Mailbox, Source};

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

/// `secrets from: file(PATH)`: a TOML file of `reference = "value"`.
///
/// The file is read on every resolution rather than held in memory, so a
/// rotated credential is picked up without a restart and a value lives in
/// this process only as long as the step that uses it.
///
/// It must not be readable by group or other. A credential file anyone on
/// the host can read is not a secret, and the failure it produces -- a
/// plan that runs perfectly while the value is public -- is silent, so it
/// is refused loudly here instead.
pub struct FileSource {
    path: std::path::PathBuf,
}

impl FileSource {
    pub fn new(path: impl Into<std::path::PathBuf>) -> FileSource {
        FileSource { path: path.into() }
    }

    #[cfg(unix)]
    fn refuse_if_readable_by_others(&self) -> Result<(), ExecError> {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&self.path)
            .map_err(|e| ExecError::Failed(format!("{}: {e}", self.path.display())))?;
        let mode = meta.permissions().mode() & 0o077;
        if mode != 0 {
            return Err(ExecError::Failed(format!(
                "{} is mode {:04o}: a secrets file readable by group or other is not a secret",
                self.path.display(),
                meta.permissions().mode() & 0o7777
            )));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn refuse_if_readable_by_others(&self) -> Result<(), ExecError> {
        // The equivalent is an ACL check, which waits for Phase 3W with
        // the rest of the Windows access-control work.
        Ok(())
    }
}

impl Source for FileSource {
    fn name(&self) -> &str {
        "file"
    }

    fn resolve(&mut self, reference: &str) -> Result<String, ExecError> {
        self.refuse_if_readable_by_others()?;
        let text = std::fs::read_to_string(&self.path)
            .map_err(|e| ExecError::Failed(format!("{}: {e}", self.path.display())))?;
        let table: BTreeMap<String, String> = toml::from_str(&text)
            .map_err(|e| ExecError::Failed(format!("{}: {e}", self.path.display())))?;
        // A reference the file does not hold is a refusal. Answering with
        // an empty string would run the step with a blank where a
        // credential belongs, which is the one outcome nobody wants.
        table.get(reference).cloned().ok_or_else(|| {
            ExecError::Failed(format!(
                "{} holds no secret named {reference}",
                self.path.display()
            ))
        })
    }
}
