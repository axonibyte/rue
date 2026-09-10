//! `rue sdk-conform` as an operator meets it: what it exits, and what it
//! says. The suite's own judgements are tested where the suite lives
//! (sdk/rust/tests/conform.rs); this is the verb around it.

use std::process::Command;

fn rue(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rue"))
        .args(args)
        .output()
        .expect("rue runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
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
