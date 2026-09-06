//! The binary against the goldens: its bytes are the expected files', its
//! exit codes are section 6.8's.

use std::fs;
use std::path::Path;
use std::process::Command;

use rue_tenants::golden::repo_root;
use rue_tenants::{NegativeCase, TenantCase, STATE_TABLE};

fn rue(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rue"))
        .args(args)
        .output()
        .expect("rue runs")
}

fn golden(root: &Path, rel: &str) -> Vec<u8> {
    fs::read(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn check_prints_the_prose_verdict_and_exits_zero_on_a_clean_plan() {
    let root = repo_root().unwrap();
    let case = TenantCase {
        tenant: "t1",
        host: "db-01",
    };
    let out = rue(&["check", root.join(case.input()).to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        golden(&root, &format!("{}/verdict.txt", case.dir()))
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn check_json_prints_the_canonical_verdict() {
    let root = repo_root().unwrap();
    let case = TenantCase {
        tenant: "t3",
        host: "fw-01",
    };
    let out = rue(&["check", "--json", root.join(case.input()).to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        golden(&root, &format!("{}/verdict.json", case.dir()))
    );
}

#[test]
fn explain_prints_the_listing() {
    let root = repo_root().unwrap();
    let case = TenantCase {
        tenant: "t2",
        host: "node-b-manual",
    };
    let out = rue(&["explain", root.join(case.input()).to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        golden(&root, &format!("{}/explain.txt", case.dir()))
    );
}

#[test]
fn a_refused_plan_exits_one_with_its_verdict() {
    let root = repo_root().unwrap();
    let neg = NegativeCase {
        code: rue_core::diagnostics::Code::E0401,
        slug: "backstop-armed-after-reach",
    };
    let out = rue(&["check", root.join(neg.input()).to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        out.stdout,
        golden(&root, &format!("{}/verdict.txt", neg.dir()))
    );
    // explain still lists the steps, and puts the verdict on stderr.
    let out = rue(&["explain", root.join(neg.input()).to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stdout.is_empty());
    assert_eq!(
        out.stderr,
        golden(&root, &format!("{}/verdict.txt", neg.dir()))
    );
}

#[test]
fn a_missing_file_a_wrong_version_and_a_usage_error_exit_two_with_nothing_on_stdout() {
    let root = repo_root().unwrap();
    let out = rue(&[
        "check",
        root.join("tenants/nope/plan.json").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));

    let doc = golden(
        &root,
        &TenantCase {
            tenant: "t4",
            host: "site-ctl",
        }
        .input(),
    );
    let doctored =
        String::from_utf8(doc)
            .unwrap()
            .replacen("\"ir_version\": 2,", "\"ir_version\": 3,", 1);
    let tmp = std::env::temp_dir().join(format!("rue-cli-test-{}.json", std::process::id()));
    fs::write(&tmp, doctored).unwrap();
    let out = rue(&["check", tmp.to_str().unwrap()]);
    let _ = fs::remove_file(&tmp);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("version 3"));

    let out = rue(&["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn states_prints_the_transition_table() {
    let root = repo_root().unwrap();
    let out = rue(&["states"]);
    assert!(out.status.success());
    assert_eq!(out.stdout, golden(&root, STATE_TABLE));
}
