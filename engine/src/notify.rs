//! `notify via:` (docs/ROADMAP.md 7.3): where the engine says that an
//! instance is waiting on a person.
//!
//! The states that hold without a bound — `Held`, `Deferred`, `Stuck`,
//! `DriftHeld` — are re-notified on every reap pass, because the thing
//! that ends them is a human and a message that arrives once can be
//! missed. A site with no `notify via:` line gets the daemon's stderr and
//! nothing else.

use crate::executor::ExecError;

/// How loud the message is. `Waiting` is a plan that will proceed on its
/// own; `Blocked` is one that will not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Waiting,
    Blocked,
}

impl Level {
    pub fn word(self) -> &'static str {
        match self {
            Level::Waiting => "waiting",
            Level::Blocked => "blocked",
        }
    }
}

pub trait Notify: Send {
    fn name(&self) -> &str;
    fn deliver(&mut self, level: Level, subject: &str, body: &str) -> Result<(), ExecError>;
}
