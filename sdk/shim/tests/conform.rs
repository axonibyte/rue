//! The shim against `rue sdk-conform`, driving a hook that is a POSIX
//! shell script. What this proves is the claim 7.11 makes for the shim: a
//! hook needs no SDK and no JSON parser, only the ability to read a line
//! and write one.
//!
//! Unix only, because the hook it drives is an `sh` script. The shim
//! itself runs a command through whatever shell the host has.

#![cfg(unix)]

use std::time::Duration;

use rue_engine::conform::conform;
use rue_hook_proto::OPS;

const KINDS: &str = "journal,inventory,execute,probe,approval,secrets,notify,scheduler";

fn suite(kinds: &str) -> rue_engine::conform::Report {
    let command = format!(
        "{} --name shim --kinds {kinds} --command {}/tests/fixtures/conformance-command.sh",
        env!("CARGO_BIN_EXE_rue-hook"),
        env!("CARGO_MANIFEST_DIR"),
    );
    conform("shim", &command, Duration::from_millis(1500)).expect("the shim starts and registers")
}

#[test]
fn a_shell_script_behind_the_shim_passes_every_case() {
    let report = suite(KINDS);
    let failed: Vec<String> = report
        .outcomes
        .iter()
        .filter(|o| !o.passed)
        .map(|o| format!("{}: {} -- {}", o.op, o.case, o.detail))
        .collect();
    assert!(
        failed.is_empty(),
        "the shim does not pass the suite:\n{}",
        failed.join("\n")
    );
    // Every op of the protocol, not a subset that happens to be easy.
    let driven = report
        .outcomes
        .iter()
        .filter(|o| o.op != "registration" && o.op != "execute.reboot")
        .count();
    assert!(
        driven >= OPS.len(),
        "only {driven} cases ran for {} ops",
        OPS.len()
    );
}

#[test]
fn the_shim_registers_only_the_kinds_it_was_given() {
    // A hook is judged on what it registered for, so the shim must not
    // over-declare: declaring a kind the command cannot serve is how a
    // plan binds to it and then refuses at the first step, a long way from
    // where the mistake was made.
    let report = suite("probe,notify");
    assert_eq!(
        report.registration.kinds,
        vec!["probe".to_string(), "notify".to_string()]
    );
    assert!(report
        .outcomes
        .iter()
        .all(|o| !o.passed || !o.op.starts_with("approval.")));
    assert_eq!(report.failed(), 0, "{:?}", report.outcomes);
}
