//! Secrets at runtime, docs/ROADMAP.md 5.13 and 7.10.
//!
//! A secret output is delivered once, when the producing step's completion
//! is journaled: the credential is live from that instant and the step's
//! undo is what revokes it. The engine offers it to the acceptors of
//! `secrets deliver_to:` in order and drops its copy as soon as one
//! accepts. A list every acceptor declines is `applied; secret
//! undelivered` and exit 7. A secret still held when the instance reverts,
//! expires or is abandoned is dropped and journaled, and a daemon restart
//! drops every held secret at boot: nothing about a secret survives in the
//! store, only in memory and in the journal's labels.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rue_core::journal::Event as J;
use rue_core::model::Instant;

use crate::executor::ExecError;
use crate::lifecycle::{Engine, EngineError, InstanceRecord};

/// The seam between `requester()` and the channel: what the acceptor put
/// there, and whether a client is attached to take it. The engine owns the
/// type so the control handler can drain it into the reply of the verb
/// that produced the secret; the acceptor itself is a binding.
#[derive(Debug, Default)]
pub struct Attached {
    pub attached: bool,
    pub pending: BTreeMap<String, Vec<(String, String)>>,
}

#[derive(Debug, Clone, Default)]
pub struct Mailbox(pub Arc<Mutex<Attached>>);

impl Mailbox {
    pub fn new() -> Mailbox {
        Mailbox::default()
    }
    pub fn with<T>(&self, f: impl FnOnce(&mut Attached) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    /// The control handler holds this while it serves a client's verb.
    pub fn attach(&self, on: bool) {
        self.with(|a| a.attached = on);
    }
    /// Everything delivered for an instance, taken.
    pub fn drain(&self, instance: &str) -> Vec<(String, String)> {
        self.with(|a| a.pending.remove(instance).unwrap_or_default())
    }
}

/// `secrets deliver_to:` (7.3): one acceptor. `deliver` answering `true`
/// ends the delivery; the engine then holds nothing.
pub trait Acceptor: Send {
    fn name(&self) -> &str;
    /// Offer the value. `until` is the bound the plan puts on it (the
    /// wane, or the site's `max_wait` for a permanent plan); `None` is no
    /// bound the engine could compute. An acceptor with a bound of its
    /// own takes the earlier of the two, which is why it is told `now`.
    fn deliver(
        &mut self,
        instance: &str,
        label: &str,
        value: &str,
        now: Instant,
        until: Option<Instant>,
    ) -> Result<bool, ExecError>;
    /// `rue reveal`: what this acceptor holds for an instance, taken once.
    fn take(&mut self, _instance: &str) -> Option<(String, String)> {
        None
    }
    /// Drop what is held for an instance; the labels dropped.
    fn drop_for(&mut self, _instance: &str) -> Vec<String> {
        Vec::new()
    }
    /// Drop what is past its bound at `now`; the (instance, label) pairs.
    fn expire(&mut self, _now: Instant) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Drop everything: a daemon restart holds no secret across it.
    fn drop_all(&mut self) -> Vec<(String, String)> {
        Vec::new()
    }
}

impl Engine {
    /// The bound a `hold(until: :wane)` resolves to for an instance: the
    /// wane deadline of a temporary plan, the site's `max_wait` for a
    /// permanent one, and R0104 when the site declares neither (5.13).
    pub(crate) fn hold_bound(&self, rec: &InstanceRecord) -> Result<Option<Instant>, String> {
        if !rec.permanent {
            return Ok(rec.deadline);
        }
        match rec.ir.site.max_wait {
            Some(w) => Ok(Some(self.clock.now().plus(w))),
            None => Err(
                "R0104: hold(until: :wane) on a permanent plan, and the site declares no max_wait"
                    .into(),
            ),
        }
    }

    /// Deliver one secret to the first acceptor that takes it (5.13).
    /// `Ok(false)` is a list every acceptor declined.
    pub(crate) fn deliver_secret(
        &mut self,
        rec: &mut InstanceRecord,
        label: &str,
        value: &str,
    ) -> Result<bool, EngineError> {
        let until = match self.hold_bound(rec) {
            Ok(u) => u,
            Err(why) => {
                self.log(
                    rec,
                    J::SecretDropped {
                        label: label.to_string(),
                        reason: why.clone(),
                    },
                )?;
                rec.secret_undelivered = true;
                return Ok(false);
            }
        };
        let id = rec.id.clone();
        let now = self.clock.now();
        let mut taken: Option<String> = None;
        for i in 0..self.acceptors.len() {
            let name = self.acceptors[i].name().to_string();
            match self.acceptors[i].deliver(&id, label, value, now, until) {
                Ok(true) => {
                    taken = Some(name);
                    break;
                }
                Ok(false) => {}
                Err(e) => {
                    // A binding that fails is not an acceptance; the next
                    // one is offered the secret (R0302 is journaled as the
                    // refusal it is).
                    self.log(
                        rec,
                        J::SecretDropped {
                            label: label.to_string(),
                            reason: format!("R0302: {name}: {e}"),
                        },
                    )?;
                }
            }
        }
        match taken {
            Some(acceptor) => {
                self.log(
                    rec,
                    J::SecretRevealed {
                        label: label.to_string(),
                        acceptor,
                    },
                )?;
                Ok(true)
            }
            None => {
                self.log(
                    rec,
                    J::SecretUndelivered {
                        label: label.to_string(),
                    },
                )?;
                rec.secret_undelivered = true;
                self.persist(rec)?;
                Ok(false)
            }
        }
    }

    /// `rue reveal <instance>`: the secret an acceptor holds, once.
    pub fn reveal(&mut self, id: &str) -> Result<Option<(String, String)>, EngineError> {
        let rec = self.load(id)?;
        for i in 0..self.acceptors.len() {
            let name = self.acceptors[i].name().to_string();
            if let Some((label, value)) = self.acceptors[i].take(id) {
                self.log(
                    &rec,
                    J::SecretRevealed {
                        label: label.clone(),
                        acceptor: format!("{name} (revealed)"),
                    },
                )?;
                return Ok(Some((label, value)));
            }
        }
        Ok(None)
    }

    /// Drop every secret held for an instance, journaling why: a revert,
    /// an expiry or an abandon leaves none behind.
    pub(crate) fn drop_secrets(
        &mut self,
        rec: &InstanceRecord,
        reason: &str,
    ) -> Result<(), EngineError> {
        let id = rec.id.clone();
        let mut dropped: Vec<String> = Vec::new();
        for a in self.acceptors.iter_mut() {
            dropped.extend(a.drop_for(&id));
        }
        for label in dropped {
            self.log(
                rec,
                J::SecretDropped {
                    label,
                    reason: reason.to_string(),
                },
            )?;
        }
        Ok(())
    }

    /// Secrets past their bound, dropped on the reap pass.
    pub(crate) fn expire_secrets(&mut self) -> Result<Vec<String>, EngineError> {
        let now = self.clock.now();
        let mut gone: Vec<(String, String)> = Vec::new();
        for a in self.acceptors.iter_mut() {
            gone.extend(a.expire(now));
        }
        let mut said = Vec::new();
        for (instance, label) in gone {
            if let Ok(rec) = self.load(&instance) {
                self.log(
                    &rec,
                    J::SecretDropped {
                        label: label.clone(),
                        reason: "the hold's bound passed".into(),
                    },
                )?;
            }
            said.push(format!("{instance}: {label} dropped at its bound"));
        }
        Ok(said)
    }

    /// A daemon restart holds no secret across it (5.13).
    pub(crate) fn drop_secrets_at_boot(&mut self) -> Result<usize, EngineError> {
        let mut gone: Vec<(String, String)> = Vec::new();
        for a in self.acceptors.iter_mut() {
            gone.extend(a.drop_all());
        }
        let n = gone.len();
        for (instance, label) in gone {
            if let Ok(rec) = self.load(&instance) {
                self.log(
                    &rec,
                    J::SecretDropped {
                        label,
                        reason: "daemon_restart".into(),
                    },
                )?;
            }
        }
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// The fake: an acceptor that takes or declines, keeps what it took under a
// bound, and gives it up once.

#[derive(Debug, Default)]
pub struct FakeAcceptorState {
    pub accepts: bool,
    /// What it holds, by instance: label, value, and its bound.
    pub kept: BTreeMap<String, (String, String, Option<Instant>)>,
    /// Every offer, whether taken or not.
    pub offers: Vec<String>,
    /// An offer that fails outright rather than declining.
    pub fails: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FakeAcceptor {
    pub label: String,
    pub state: Arc<Mutex<FakeAcceptorState>>,
}

impl FakeAcceptor {
    /// An acceptor by name that takes what it is offered, or declines.
    pub fn new(name: &str, accepts: bool) -> FakeAcceptor {
        let a = FakeAcceptor {
            label: name.to_string(),
            state: Arc::new(Mutex::new(FakeAcceptorState::default())),
        };
        a.with(|s| s.accepts = accepts);
        a
    }
    pub fn with<T>(&self, f: impl FnOnce(&mut FakeAcceptorState) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn holds(&self, instance: &str) -> bool {
        self.with(|s| s.kept.contains_key(instance))
    }
    pub fn offers(&self) -> Vec<String> {
        self.with(|s| s.offers.clone())
    }
}

impl Acceptor for FakeAcceptor {
    fn name(&self) -> &str {
        &self.label
    }

    fn deliver(
        &mut self,
        instance: &str,
        label: &str,
        value: &str,
        _now: Instant,
        until: Option<Instant>,
    ) -> Result<bool, ExecError> {
        let (accepts, fails) = self.with(|s| {
            s.offers.push(format!("{instance} {label}"));
            (s.accepts, s.fails)
        });
        if fails {
            return Err(ExecError::Failed("the acceptor broke".into()));
        }
        if !accepts {
            return Ok(false);
        }
        self.with(|s| {
            s.kept.insert(
                instance.to_string(),
                (label.to_string(), value.to_string(), until),
            );
        });
        Ok(true)
    }

    fn take(&mut self, instance: &str) -> Option<(String, String)> {
        self.with(|s| s.kept.remove(instance).map(|(l, v, _)| (l, v)))
    }

    fn drop_for(&mut self, instance: &str) -> Vec<String> {
        self.with(|s| {
            s.kept
                .remove(instance)
                .map(|(l, _, _)| vec![l])
                .unwrap_or_default()
        })
    }

    fn expire(&mut self, now: Instant) -> Vec<(String, String)> {
        self.with(|s| {
            let gone: Vec<String> = s
                .kept
                .iter()
                .filter(|(_, (_, _, u))| u.is_some_and(|u| now.unix_s >= u.unix_s))
                .map(|(i, _)| i.clone())
                .collect();
            gone.into_iter()
                .filter_map(|i| s.kept.remove(&i).map(|(l, _, _)| (i, l)))
                .collect()
        })
    }

    fn drop_all(&mut self) -> Vec<(String, String)> {
        self.with(|s| {
            std::mem::take(&mut s.kept)
                .into_iter()
                .map(|(i, (l, _, _))| (i, l))
                .collect()
        })
    }
}
