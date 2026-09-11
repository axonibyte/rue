//! examples/audit.sh behind the shim, the quick start of docs/README.md,
//! does what the page says it does; and the shim skips a line that is not
//! JSON rather than ending. Unix only, because the hook is an `sh` script.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

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
            "rue-shim-audit-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Scratch(p)
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

struct Shim {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Shim {
    /// The shim serving examples/audit.sh, acknowledged; its registration.
    fn start(log: &std::path::Path) -> (Shim, Value) {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/audit.sh");
        let mut child = Command::new(env!("CARGO_BIN_EXE_rue-hook"))
            .args(["--name", "audit", "--kinds", "journal,notify", "--command"])
            .arg(format!("sh {script}"))
            .env("RUE_AUDIT_LOG", log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut shim = Shim {
            child,
            stdin,
            stdout,
        };
        let registration = shim.line();
        shim.send(r#"{"register":{"ok":true}}"#);
        (shim, registration)
    }

    fn send(&mut self, line: &str) {
        let w = self.stdin.as_mut().unwrap();
        w.write_all(line.as_bytes()).unwrap();
        w.write_all(b"\n").unwrap();
        w.flush().unwrap();
    }

    fn line(&mut self) -> Value {
        let mut text = String::new();
        assert!(
            self.stdout.read_line(&mut text).unwrap() > 0,
            "the shim ended"
        );
        serde_json::from_str(text.trim_end()).unwrap()
    }

    fn ask(&mut self, request: Value) -> Value {
        self.send(&request.to_string());
        self.line()
    }
}

impl Drop for Shim {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

#[test]
fn it_registers_for_journal_and_notify_and_appends_each_entry() {
    let dir = Scratch::new("append");
    let log = dir.0.join("audit.ndjson");
    let (mut shim, registration) = Shim::start(&log);
    assert_eq!(registration["register"]["name"], json!("audit"));
    assert_eq!(
        registration["register"]["kinds"],
        json!(["journal", "notify"])
    );
    for seq in [1, 2] {
        let reply = shim.ask(json!({ "id": seq, "kind": "journal", "op": "append",
                                     "entry": { "seq": seq } }));
        assert_eq!(reply, json!({ "id": seq, "ok": true }));
    }
    let reply = shim.ask(json!({ "id": 3, "kind": "notify", "op": "deliver",
                                 "level": "warn", "subject": "s", "body": "b" }));
    assert_eq!(reply, json!({ "id": 3, "ok": true }));
    drop(shim);
    let seqs: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["entry"]["seq"].clone())
        .collect();
    assert_eq!(seqs, vec![json!(1), json!(2)]);
}

#[test]
fn an_entry_it_cannot_record_is_refused_with_the_reason() {
    let dir = Scratch::new("refuse");
    let (mut shim, _) = Shim::start(&dir.0.join("no-such-dir").join("audit.ndjson"));
    let reply = shim.ask(json!({ "id": 1, "kind": "journal", "op": "append",
                                 "entry": { "seq": 1 } }));
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert!(
        reply["error"].as_str().unwrap().contains("not writable"),
        "{reply}"
    );
}

#[test]
fn a_line_that_is_not_json_is_skipped_rather_than_ending_the_shim() {
    let dir = Scratch::new("badline");
    let (mut shim, _) = Shim::start(&dir.0.join("audit.ndjson"));
    shim.send("not json at all");
    shim.send(&"[".repeat(10_000));
    let reply = shim.ask(json!({ "id": 7, "kind": "notify", "op": "deliver",
                                 "level": "info", "subject": "s", "body": "b" }));
    assert_eq!(reply, json!({ "id": 7, "ok": true }));
}
