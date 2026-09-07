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
            .replacen("\"ir_version\": 3,", "\"ir_version\": 4,", 1);
    let tmp = std::env::temp_dir().join(format!("rue-cli-test-{}.json", std::process::id()));
    fs::write(&tmp, doctored).unwrap();
    let out = rue(&["check", tmp.to_str().unwrap()]);
    let _ = fs::remove_file(&tmp);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("version 4"));

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

#[test]
fn artifact_prints_the_golden_and_reports_refusals_by_exit_code() {
    let root = repo_root().unwrap();
    // Byte for byte the golden, for a sh, a PowerShell and a Python case.
    for (tenant, host, file) in [
        ("t1", "db-01", "artifact.sh"),
        ("t3", "fw-win-01", "artifact.ps1"),
        ("t3", "fw-02", "artifact.py"),
    ] {
        let case = TenantCase { tenant, host };
        let out = rue(&[
            "artifact",
            root.join(case.input()).to_str().unwrap(),
            "--instance",
            "golden",
            "--set",
            "port=8443",
        ]);
        assert!(
            out.status.success(),
            "{tenant}/{host}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let expected = fs::read(root.join(case.dir()).join(file)).unwrap();
        assert_eq!(out.stdout, expected, "{tenant}/{host}");
    }
    // --host selects another record of the site; --rue-root is baked.
    let t3 = TenantCase {
        tenant: "t3",
        host: "fw-01",
    };
    let out = rue(&[
        "artifact",
        root.join(t3.input()).to_str().unwrap(),
        "--instance",
        "i-9",
        "--host",
        "fw-mac-01",
        "--rue-root",
        "/opt/rue",
    ]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("on fw-mac-01 (os macos)") && text.contains("INST='/opt/rue/instances/i-9'")
    );

    // A diagnostic (E0403: sh declared on a Windows host) is exit 1.
    let neg = root.join("tenants/_negative/E0403-artifact-language-unsupported/expected/plan.json");
    let out = rue(&["artifact", neg.to_str().unwrap(), "--instance", "x"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr)
        .contains(&rue_core::diagnostics::Code::E0403.to_string()));

    // A call that cannot apply (no :target backstop; an unknown host; a bad
    // --set) is exit 2 with nothing on stdout.
    let t4 = TenantCase {
        tenant: "t4",
        host: "site-ctl",
    };
    let out = rue(&[
        "artifact",
        root.join(t4.input()).to_str().unwrap(),
        "--instance",
        "x",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let out = rue(&[
        "artifact",
        root.join(t3.input()).to_str().unwrap(),
        "--instance",
        "x",
        "--host",
        "nope",
    ]);
    assert_eq!(out.status.code(), Some(2));
    let out = rue(&[
        "artifact",
        root.join(t3.input()).to_str().unwrap(),
        "--instance",
        "x",
        "--set",
        "port",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("NAME=VALUE"));
}

#[test]
fn fmt_prints_the_canonical_text_checks_it_and_refuses_a_file_with_errors() {
    let root = repo_root().unwrap();
    let t1 = root.join("tenants/t1/plan.rue");
    let out = rue(&["fmt", t1.to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        fs::read(&t1).unwrap(),
        "fmt is the identity on a tenant file"
    );
    let out = rue(&["fmt", "--check", t1.to_str().unwrap()]);
    assert!(out.status.success() && out.stdout.is_empty());

    let tmp = std::env::temp_dir().join(format!("rue-fmt-{}.rue", std::process::id()));
    fs::write(
        &tmp,
        "rue 0\ndefplan :p,%{name: \"db-01\"} do\n    wane  1h\nend\n",
    )
    .unwrap();
    let out = rue(&["fmt", "--check", tmp.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not formatted"));
    let out = rue(&["fmt", tmp.to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "rue 0\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\nend\n"
    );

    fs::write(
        &tmp,
        "rue 0\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n",
    )
    .unwrap();
    let out = rue(&["fmt", tmp.to_str().unwrap()]);
    let _ = fs::remove_file(&tmp);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "a file with errors is never rewritten"
    );
    assert!(String::from_utf8_lossy(&out.stderr)
        .contains(&rue_core::diagnostics::Code::E0101.to_string()));
}
