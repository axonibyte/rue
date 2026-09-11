//! examples/audit_hook.rs, the quick start of docs/README.md, does what the
//! page says it does.

#[path = "../examples/audit_hook.rs"]
mod audit_hook;

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// A directory of the test's own, removed when the test ends; a directory
/// that cannot be removed fails the test rather than being left behind.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "rue-sdk-audit-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Scratch(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            if !std::thread::panicking() {
                panic!("{} was not removed: {e}", self.0.display());
            }
        }
    }
}

fn request(kind: &str, op: &str, fields: Value) -> Value {
    let mut r = json!({ "id": 1, "kind": kind, "op": op });
    r.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    r
}

#[test]
fn it_registers_for_journal_and_notify() {
    let dir = Scratch::new("kinds");
    let hooks = audit_hook::hooks(dir.path().join("audit.ndjson"));
    assert_eq!(hooks.kinds(), vec!["journal", "notify"]);
}

#[test]
fn each_entry_is_appended_as_one_line() {
    let dir = Scratch::new("append");
    let log = dir.path().join("audit.ndjson");
    let mut hooks = audit_hook::hooks(log.clone());
    for seq in [1, 2] {
        let reply = hooks.answer(&request(
            "journal",
            "append",
            json!({ "entry": { "seq": seq } }),
        ));
        assert_eq!(reply, json!({ "id": 1, "ok": true }));
    }
    let text = std::fs::read_to_string(&log).unwrap();
    let seqs: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["seq"].clone())
        .collect();
    assert_eq!(seqs, vec![json!(1), json!(2)]);
}

#[test]
fn an_entry_it_cannot_record_is_refused_with_the_reason() {
    let dir = Scratch::new("refuse");
    let mut hooks = audit_hook::hooks(dir.path().join("no-such-dir").join("audit.ndjson"));
    let reply = hooks.answer(&request(
        "journal",
        "append",
        json!({ "entry": { "seq": 1 } }),
    ));
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert!(
        reply["error"].as_str().unwrap().contains("is not writable"),
        "{reply}"
    );
}

#[test]
fn a_notification_is_acknowledged() {
    let dir = Scratch::new("notify");
    let mut hooks = audit_hook::hooks(dir.path().join("audit.ndjson"));
    let reply = hooks.answer(&request(
        "notify",
        "deliver",
        json!({ "level": "warn", "subject": "plan held", "body": "waiting for approval" }),
    ));
    assert_eq!(reply, json!({ "id": 1, "ok": true }));
}

/// The example's `main` is what `rued` runs; it serves these hooks on stdio.
#[test]
fn main_is_the_stdio_entry_point() {
    let _: fn() -> std::io::Result<()> = audit_hook::main;
}
