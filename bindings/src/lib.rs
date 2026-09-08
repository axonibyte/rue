//! rue-bindings: the generic built-ins of docs/ROADMAP.md section 7.3, one
//! per binding kind and no more. Everything tenant-specific is a hook. The
//! crate arrives by unit: this one brings the journal sinks `file(path)`
//! and `stdout()` (7.6) and the signing key `key(path)` (5.10).

pub mod approval;
pub mod cron;
pub mod journal;
pub mod launchd;
pub mod local;
pub mod notify;
pub mod secrets;
pub mod ssh;
pub mod task_scheduler;
