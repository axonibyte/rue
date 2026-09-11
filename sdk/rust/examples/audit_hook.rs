//! An audit hook: a journal sink that keeps every entry, and a notifier.
//!
//! Bind it in a site with `journal to: local(), hook(:audit)` and
//! `notify via: hook(:audit)`, and have rued spawn it:
//!
//!     rued run --spawn audit=/usr/local/libexec/audit_hook ...
//!
//! Each journal entry is appended to $RUE_AUDIT_LOG (default audit.ndjson)
//! as one line of JSON. A sink that cannot record an entry must say so: the
//! engine then refuses to proceed (R0304) rather than run a step nobody
//! recorded. Notifications go to stderr, because stdout carries the protocol.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use rue_hook_sdk::{serve_stdio, Answer, Hooks, Journal, Notify, Refusal, ServeOptions};
use serde_json::Value;

pub struct AuditLog {
    pub path: PathBuf,
}

impl Journal for AuditLog {
    fn append(&mut self, entry: &Value) -> Answer<()> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(format!("{entry}\n").as_bytes()))
            .map_err(|e| {
                let path = self.path.display();
                Refusal::new(format!("the audit log {path} is not writable: {e}"))
            })
    }
}

pub struct Stderr;

impl Notify for Stderr {
    fn deliver(&mut self, level: &str, subject: &str, body: &str) -> Answer<()> {
        eprintln!("[{level}] {subject}: {body}");
        Ok(())
    }
}

pub fn hooks(path: PathBuf) -> Hooks {
    let mut hooks = Hooks::new();
    hooks.journal = Some(Box::new(AuditLog { path }));
    hooks.notify = Some(Box::new(Stderr));
    hooks
}

// `pub` only so that the SDK's own tests can include this file and name it.
pub fn main() -> std::io::Result<()> {
    let path =
        std::env::var_os("RUE_AUDIT_LOG").map_or_else(|| "audit.ndjson".into(), PathBuf::from);
    serve_stdio(hooks(path), ServeOptions::new("audit"))
}
