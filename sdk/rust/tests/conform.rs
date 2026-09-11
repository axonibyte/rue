//! This SDK against `rue sdk-conform`'s own suite: the reference hook is
//! spawned exactly as `rued` spawns a `--spawn` child and driven through
//! every op of docs/ROADMAP.md 7.5.
//!
//! The second test is the one that keeps the suite honest: the ops the
//! runner actually drove must be exactly the ops of `rue-hook-proto`'s
//! table. An op added to the protocol with no case is a hole nothing else
//! would notice -- `tools/lint-hook-ops.sh` binds the table to the
//! document, and this binds it to the cases.

use std::collections::BTreeSet;
use std::time::Duration;

use rue_engine::conform::conform;
use rue_hook_proto::OPS;

fn suite() -> rue_engine::conform::Report {
    conform(
        "conform",
        &format!("{} conform", env!("CARGO_BIN_EXE_rue-conform-hook")),
        Duration::from_millis(1500),
    )
    .expect("the reference hook starts and registers")
}

#[test]
fn the_reference_hook_passes_every_case() {
    let report = suite();
    let failed: Vec<String> = report
        .outcomes
        .iter()
        .filter(|o| !o.passed)
        .map(|o| format!("{}: {} -- {}", o.op, o.case, o.detail))
        .collect();
    assert!(
        failed.is_empty(),
        "the reference SDK does not pass its own suite:\n{}",
        failed.join("\n")
    );
    assert!(
        report.passed() > 30,
        "only {} cases ran; the suite is not driving the protocol",
        report.passed()
    );
    assert_eq!(report.registration.kinds.len(), 8, "every kind is served");
}

#[test]
fn the_suite_drives_every_op_of_the_protocol() {
    let report = suite();
    let driven: BTreeSet<&str> = report
        .outcomes
        .iter()
        .map(|o| o.op.as_str())
        .filter(|o| *o != "registration" && *o != "execute.reboot")
        .collect();
    let rows: BTreeSet<String> = OPS.iter().map(|o| format!("{}.{}", o.kind, o.op)).collect();
    let rows: BTreeSet<&str> = rows.iter().map(String::as_str).collect();
    let undriven: Vec<&&str> = rows.difference(&driven).collect();
    assert!(
        undriven.is_empty(),
        "these ops have a row in the protocol and no conformance case: {undriven:?}"
    );
    let unknown: Vec<&&str> = driven.difference(&rows).collect();
    assert!(
        unknown.is_empty(),
        "these cases name an op the protocol has no row for: {unknown:?}"
    );
}

/// The reference conformance hook skips a line that is not JSON, as the
/// SDK's serve loop does, rather than ending: it is the example every other
/// SDK's conformance hook is written to match.
#[test]
fn the_conformance_hook_skips_a_line_that_is_not_json() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let mut child = Command::new(env!("CARGO_BIN_EXE_rue-conform-hook"))
        .arg("conform")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert!(line.contains("\"register\""), "{line}");
    stdin
        .write_all(
            b"{\"register\":{\"ok\":true}}\nnot json at all\n\
              {\"id\":7,\"kind\":\"journal\",\"op\":\"append\",\"entry\":{}}\n",
        )
        .unwrap();
    stdin.flush().unwrap();
    line.clear();
    out.read_line(&mut line).unwrap();
    drop(stdin);
    let _ = child.wait();
    let reply: serde_json::Value = serde_json::from_str(line.trim_end())
        .unwrap_or_else(|e| panic!("the hook ended or wrote something else: {e}: {line:?}"));
    assert_eq!(reply["id"], serde_json::json!(7), "{reply}");
    assert_eq!(reply["ok"], serde_json::json!(true), "{reply}");
}
