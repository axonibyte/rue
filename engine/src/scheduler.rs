//! The `backstop scheduler` binding, docs/ROADMAP.md 7.7 and Appendix C:
//! the target-side entry that runs a rendered artifact until it is
//! disarmed.
//!
//! The division of labor between the engine and this binding is the one
//! 7.7 states. The engine owns the instance directory: it writes the
//! artifact, the `deadline` and the `heartbeat` through the executor, and
//! it is the engine that probes the target's clock before writing a
//! deadline (R0403). The binding owns the scheduler entry: `install`
//! creates it, `disarm` removes it, `present` answers whether it is still
//! there, and `arm`/`rearm` are the binding's chance to carry a deadline
//! the scheduler itself enforces.
//!
//! For a periodic scheduler like `cron()` the artifact is self-enforcing:
//! the entry runs it on the family's granularity and the artifact compares
//! the deadline file itself, so `arm` and `rearm` have nothing of their
//! own to do. A scheduler that holds the time itself, like Task Scheduler,
//! sets its trigger there instead. Both shapes are honest under this
//! trait, and `rue explain` says which one a host has.

use std::fmt;

use rue_core::model::{ArtifactLanguage, Instant};

use crate::executor::{ExecError, Executor};
use crate::host::Host;

/// What the scheduler runs, and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub instance: String,
    /// The artifact's absolute path in the instance directory.
    pub artifact: String,
    pub language: ArtifactLanguage,
    /// The host's `os` word, which decides the family's invocation.
    pub os: String,
}

impl Job {
    /// The command the scheduler entry runs: the artifact under the
    /// interpreter its language needs (4.5).
    pub fn command(&self) -> String {
        match self.language {
            ArtifactLanguage::Sh => format!("/bin/sh {}", self.artifact),
            ArtifactLanguage::Python => format!("uv run --offline --script {}", self.artifact),
            ArtifactLanguage::Powershell => {
                format!("powershell -NoProfile -File {}", self.artifact)
            }
        }
    }
}

/// Whether the entry is there. `Unknown` is a scheduler that cannot say,
/// and it is never read as absence: reclaim refuses on anything but a
/// stated absence (R0405).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Present,
    Absent,
    Unknown,
}

impl fmt::Display for Presence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Presence::Present => "present",
            Presence::Absent => "absent",
            Presence::Unknown => "unknown",
        })
    }
}

/// The binding. Every op acts on the host through the executor that
/// reaches it, so a scheduler binding needs no transport of its own.
pub trait Scheduler: Send {
    /// The name the site's `backstop scheduler:` binding and a host record
    /// use (`cron`, `task_scheduler`, `launchd`, a hook's name).
    fn name(&self) -> &str;
    /// Create the entry that runs the artifact.
    fn install(&mut self, ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError>;
    /// The deadline is now live. A periodic scheduler has nothing to do.
    fn arm(
        &mut self,
        ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
        deadline: Instant,
    ) -> Result<(), ExecError>;
    /// The deadline moved. Nothing else about the entry changes.
    fn rearm(
        &mut self,
        ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
        deadline: Instant,
    ) -> Result<(), ExecError>;
    /// Remove the entry.
    fn disarm(&mut self, ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError>;
    /// Is the entry there?
    fn present(
        &mut self,
        ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
    ) -> Result<Presence, ExecError>;
}

// ---------------------------------------------------------------------------
// The fake: every op recorded, any op scriptable to fail.

use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub struct FakeScheduler {
    pub ops: Vec<String>,
    /// Ops that fail, by name (`install`, `arm`, `rearm`, `disarm`,
    /// `present`).
    pub fails: Vec<String>,
    pub presence: Option<Presence>,
    /// Entries this scheduler holds, by (host, instance).
    pub entries: Vec<(String, String)>,
}

/// A handle the engine holds as `Box<dyn Scheduler>` and a test reads.
#[derive(Debug, Clone, Default)]
pub struct FakeSchedulerHandle(pub Arc<Mutex<FakeScheduler>>);

impl FakeSchedulerHandle {
    pub fn new() -> FakeSchedulerHandle {
        FakeSchedulerHandle::default()
    }
    pub fn with<T>(&self, f: impl FnOnce(&mut FakeScheduler) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn ops(&self) -> Vec<String> {
        self.with(|f| f.ops.clone())
    }
    pub fn fail(&self, op: &str) {
        self.with(|f| f.fails.push(op.to_string()));
    }
    pub fn entries(&self) -> Vec<(String, String)> {
        self.with(|f| f.entries.clone())
    }
    fn note(&self, op: &str, host: &Host, job: &Job) -> Result<(), ExecError> {
        self.with(|f| {
            f.ops.push(format!("{op} {} {}", host.name(), job.instance));
            if f.fails.iter().any(|x| x == op) {
                return Err(ExecError::Failed(format!("{op} refused by the fake")));
            }
            let key = (host.name().to_string(), job.instance.clone());
            match op {
                "install" => {
                    if !f.entries.contains(&key) {
                        f.entries.push(key);
                    }
                }
                "disarm" => f.entries.retain(|e| e != &key),
                _ => {}
            }
            Ok(())
        })
    }
}

impl Scheduler for FakeSchedulerHandle {
    fn name(&self) -> &str {
        "cron"
    }
    fn install(&mut self, _ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError> {
        self.note("install", host, job)
    }
    fn arm(
        &mut self,
        _ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
        _deadline: Instant,
    ) -> Result<(), ExecError> {
        self.note("arm", host, job)
    }
    fn rearm(
        &mut self,
        _ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
        _deadline: Instant,
    ) -> Result<(), ExecError> {
        self.note("rearm", host, job)
    }
    fn disarm(&mut self, _ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError> {
        self.note("disarm", host, job)
    }
    fn present(
        &mut self,
        _ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
    ) -> Result<Presence, ExecError> {
        self.note("present", host, job)?;
        Ok(self.with(|f| {
            f.presence.unwrap_or({
                let key = (host.name().to_string(), job.instance.clone());
                if f.entries.contains(&key) {
                    Presence::Present
                } else {
                    Presence::Absent
                }
            })
        }))
    }
}
