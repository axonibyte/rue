//! `rue sdk-conform` as an operator meets it: what it exits, and what it
//! says. The suite's own judgements are tested where the suite lives
//! (sdk/rust/tests/conform.rs); this is the verb around it.
//!
//! Unix only, like cli/tests/daemon.rs beside it: the hook these cases
//! judge is `tests/fixtures/stub-hook.sh`, a POSIX shell script, and there
//! is no `sh` to run it on Windows. What is scoped here is the fixture,
//! not the verb -- `rue sdk-conform` runs a hook through whatever shell
//! the host has (`rue_engine::hook::host_shell`).

#![cfg(unix)]

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The exit code and stdout, with stderr folded into the text so a failed
/// assertion on the code says what went wrong. An unexpected exit is
/// exactly when the reason matters, and the reason is on stderr:
/// `rue sdk-conform` exits 2 without judging anything when it cannot start
/// the hook, and says why there.
fn rue(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rue"))
        .args(args)
        .output()
        .expect("rue runs");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        text.push_str("\n--- stderr ---\n");
        text.push_str(&err);
    }
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn a_hook_that_answers_ok_to_everything_fails_the_suite_and_says_which_cases() {
    // The sh stub of cli/tests/fixtures serves `execute` and answers a
    // bare ok to every op. That is enough to run a plan and not enough to
    // be an SDK: it cannot say it does not serve something, and it
    // promises fields it does not send.
    let stub = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stub-hook.sh");
    let (code, out) = rue(&["sdk-conform", "--name", "act", &format!("sh {stub} act")]);
    assert_eq!(code, 1, "a hook with failures exits 1:\n{out}");
    assert!(
        out.contains("not ok  execute.clock"),
        "an ok without epoch_s is R0303 and must be named:\n{out}"
    );
    assert!(
        out.contains("not ok  execute.reboot"),
        "an op the protocol has no row for must be refused, not answered:\n{out}"
    );
    assert!(out.contains("passed,") && out.contains("failed,"), "{out}");
}

#[test]
fn a_hook_that_never_registers_is_exit_two_because_nothing_was_judged() {
    // Distinct from exit 1: a hook that ran and failed cases has been
    // judged, and one that never started has not.
    let (code, _) = rue(&["sdk-conform", "--name", "nothing", "exit 0"]);
    assert_eq!(code, 2);
    let (code, _) = rue(&["sdk-conform", "--name", "nothing", "echo not-a-frame"]);
    assert_eq!(code, 2);
}

#[test]
fn the_json_report_carries_every_case_and_the_registration() {
    let stub = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stub-hook.sh");
    let (code, out) = rue(&[
        "sdk-conform",
        "--json",
        "--name",
        "act",
        &format!("sh {stub} act"),
    ]);
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_str(&out).expect("a JSON report");
    assert_eq!(v["name"], serde_json::json!("act"));
    assert_eq!(v["registration"]["kinds"], serde_json::json!(["execute"]));
    assert!(v["failed"].as_u64().unwrap() > 0);
    let cases = v["cases"].as_array().expect("cases");
    assert!(cases
        .iter()
        .all(|c| c["op"].is_string() && c["passed"].is_boolean()));
    // A kind the hook never registered is never asked about.
    assert!(
        !cases
            .iter()
            .any(|c| c["op"].as_str().unwrap_or("").starts_with("approval.")),
        "a hook is judged only on what it registered for"
    );
}

/// Run `rue` and refuse to wait forever: a hang is reported as a failure
/// with something to read, not as a wedged suite.
fn rue_bounded(args: &[&str], bound: Duration) -> Option<(i32, String)> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rue"))
        .args(args)
        // Both are some four kilobytes, well inside a pipe buffer, so they
        // are read after the exit rather than concurrently.
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rue runs");
    let until = Instant::now() + bound;
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            let mut out = String::new();
            if let Some(mut o) = child.stdout.take() {
                let _ = o.read_to_string(&mut out);
            }
            if let Some(mut e) = child.stderr.take() {
                let mut err = String::new();
                let _ = e.read_to_string(&mut err);
                if !err.trim().is_empty() {
                    out.push_str("\n--- stderr ---\n");
                    out.push_str(&err);
                }
            }
            return Some((status.code().unwrap_or(-1), out));
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_hook_the_shell_did_not_exec_still_ends_when_the_suite_does() {
    // `sh -c <command>` need not run the hook in the process it returns:
    // the trailing no-op here guarantees a fork, and it is what a hook
    // started through a wrapper looks like in the wild. Signalling the
    // process we spawned then reaps the shell and leaves the hook an
    // orphan holding the stdout pipe, and whoever reads that pipe waits
    // for an end that never comes -- which is a hang, not a failure, and
    // so the worst way for this to go wrong. Closing the hook's stdin ends
    // whichever process is really serving.
    let stub = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stub-hook.sh");
    let answered = rue_bounded(
        &["sdk-conform", "--name", "act", &format!("sh {stub} act; :")],
        Duration::from_secs(30),
    );
    let Some((code, out)) = answered else {
        panic!(
            "the suite did not end within 30s: a hook the shell did not exec was left holding \
             the pipe"
        )
    };
    assert_eq!(code, 1, "{out}");
}
