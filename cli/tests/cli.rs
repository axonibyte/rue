//! The binary against the goldens: its bytes are the expected files', its
//! exit codes are section 6.8's.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rue_tenants::golden::repo_root;
use rue_tenants::{NegativeCase, TenantCase, NEGATIVES, STATE_TABLE, TENANT_CASES};

/// A tenant case from the table, by tenant and case directory.
fn tenant_case(tenant: &str, host: &str) -> TenantCase {
    *TENANT_CASES
        .iter()
        .find(|c| c.tenant == tenant && c.host == host)
        .unwrap_or_else(|| panic!("no case {tenant}/{host}"))
}

/// A negative from the table, by code and slug.
fn negative_case(code: rue_core::diagnostics::Code, slug: &str) -> NegativeCase {
    *NEGATIVES
        .iter()
        .find(|n| n.code == code && n.slug == slug)
        .unwrap_or_else(|| panic!("no negative {code}-{slug}"))
}

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
    let case = tenant_case("t1", "db-01");
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
    let case = tenant_case("t3", "fw-01");
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
    let case = tenant_case("t2", "node-b-manual");
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
    let neg = negative_case(
        rue_core::diagnostics::Code::E0401,
        "backstop-armed-after-reach",
    );
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

    let doc = golden(&root, &tenant_case("t4", "site-ctl").input());
    let current = rue_core::ir::IR_VERSION;
    let doctored = String::from_utf8(doc).unwrap().replacen(
        &format!("\"ir_version\": {current},"),
        &format!("\"ir_version\": {},", current + 1),
        1,
    );
    let tmp = std::env::temp_dir().join(format!("rue-cli-test-{}.json", std::process::id()));
    fs::write(&tmp, doctored).unwrap();
    let out = rue(&["check", tmp.to_str().unwrap()]);
    let _ = fs::remove_file(&tmp);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains(&format!("version {}", current + 1)));

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
        let case = tenant_case(tenant, host);
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
    let t3 = tenant_case("t3", "fw-01");
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
    let t4 = tenant_case("t4", "site-ctl");
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

#[test]
fn check_explain_and_artifact_read_a_rue_file_for_one_host() {
    let root = repo_root().unwrap();
    let t1 = root.join("tenants/t1/plan.rue");
    let out = rue(&["check", t1.to_str().unwrap(), "--json", "--host", "db-01"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        fs::read(root.join("tenants/t1/expected/db-01/verdict.json")).unwrap()
    );
    // The plan's pattern names its host, so --host may be omitted; the
    // requester is the first declared operator.
    let out = rue(&["check", t1.to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        fs::read(root.join("tenants/t1/expected/db-01/verdict.txt")).unwrap()
    );
    let out = rue(&["explain", t1.to_str().unwrap()]);
    assert_eq!(
        out.stdout,
        fs::read(root.join("tenants/t1/expected/db-01/explain.txt")).unwrap()
    );
    let out = rue(&["artifact", t1.to_str().unwrap(), "--instance", "golden"]);
    assert_eq!(
        out.stdout,
        fs::read(root.join("tenants/t1/expected/db-01/artifact.sh")).unwrap()
    );

    // A file with two plans needs --plan-name; a clause-dispatched plan
    // needs --host.
    let t2 = root.join("tenants/t2/plan.rue");
    let out = rue(&["check", t2.to_str().unwrap(), "--host", "node-b"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--plan"));
    let out = rue(&[
        "check",
        t2.to_str().unwrap(),
        "--json",
        "--host",
        "node-b",
        "--plan-name",
        "promote_auto",
    ]);
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        fs::read(root.join("tenants/t2/expected/node-b-auto/verdict.json")).unwrap()
    );
    let t3 = root.join("tenants/t3/plan.rue");
    let out = rue(&["check", t3.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--host"));
    let out = rue(&[
        "check",
        t3.to_str().unwrap(),
        "--json",
        "--host",
        "fw-mac-01",
    ]);
    assert_eq!(
        out.stdout,
        fs::read(root.join("tenants/t3/expected/fw-mac-01/verdict.json")).unwrap()
    );

    // A negative refuses with exit 1 and its verdict; an unknown host is a
    // diagnostic with a suggestion.
    let neg = root.join("tenants/_negative/E0301-umbra-conflict/plan.rue");
    let out = rue(&["check", neg.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        out.stdout,
        fs::read(root.join("tenants/_negative/E0301-umbra-conflict/expected/verdict.txt")).unwrap()
    );
    let out = rue(&["check", t1.to_str().unwrap(), "--host", "db-1"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&rue_core::diagnostics::Code::E0102.to_string())
            && err.contains("did you mean db-01"),
        "{err}"
    );

    // Selectors on a plan IR are a usage error.
    let out = rue(&[
        "check",
        root.join("tenants/t1/expected/db-01/plan.json")
            .to_str()
            .unwrap(),
        "--host",
        "db-01",
    ]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_diagnostic_on_stderr_shows_the_code_the_source_line_and_the_suggestion() {
    let root = repo_root().unwrap();
    let neg = root.join("tenants/_negative/E0102-unknown-op/plan.rue");
    let out = rue(&["check", neg.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&rue_core::diagnostics::Code::E0102.to_string()),
        "{err}"
    );
    assert!(err.contains("postrue()"), "the source line is shown: {err}");
    assert!(err.contains("did you mean posture"), "{err}");
    // The file and the position, whichever separator the platform writes.
    assert!(
        err.contains(&format!(
            "{}-unknown-op",
            rue_core::diagnostics::Code::E0102
        )) && err.contains("plan.rue:22:3"),
        "{err}"
    );
}

// --- rue journal verify ------------------------------------------------------

fn journal_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "rue-cli-journal-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn write_chain(path: &std::path::Path, entries: &[rue_core::journal::Entry]) {
    let mut text = String::new();
    for e in entries {
        text.push_str(&serde_json::to_string(e).unwrap());
        text.push('\n');
    }
    fs::write(path, text).unwrap();
}

#[test]
fn journal_verify_accepts_a_chain_names_a_broken_link_and_refuses_an_empty_file() {
    use rue_core::journal::{append, Event};
    use rue_core::model::Instant;
    let d = journal_dir("chain");
    let e1 = append(&[], Instant::new(1), "p", "i", "h", Event::Checked, vec![]);
    let e2 = append(
        std::slice::from_ref(&e1),
        Instant::new(2),
        "p",
        "i",
        "h",
        Event::Requested,
        vec![],
    );
    let file = d.join("journal.ndjson");
    write_chain(&file, &[e1.clone(), e2.clone()]);
    let out = rue(&["journal", "verify", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("2 entries, chain verified"), "{text}");

    let mut broken = e2.clone();
    broken.prev_hash = rue_core::journal::Hash::ZERO;
    write_chain(&file, &[e1, broken]);
    let out = rue(&["journal", "verify", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("entry 2") && err.contains("prev_hash"),
        "{err}"
    );
    assert!(out.stdout.is_empty());

    fs::write(&file, "").unwrap();
    let out = rue(&["journal", "verify", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no entries"));

    let out = rue(&["journal", "verify", d.join("missing").to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a missing file is an empty chain: refused"
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn journal_verify_with_a_key_checks_every_signature_and_refuses_an_unsigned_entry() {
    use rue_core::journal::{append, Event};
    use rue_core::model::Instant;
    let d = journal_dir("signed");
    let key = d.join("id_ed25519");
    let signer = rue_engine::sign::generate(&key).unwrap();
    let mut e1 = append(&[], Instant::new(1), "p", "i", "h", Event::Checked, vec![]);
    e1.sig = Some(signer.sign(&e1).unwrap());
    let mut e2 = append(
        std::slice::from_ref(&e1),
        Instant::new(2),
        "p",
        "i",
        "h",
        Event::Requested,
        vec![],
    );
    e2.sig = Some(signer.sign(&e2).unwrap());
    let file = d.join("journal.ndjson");
    write_chain(&file, &[e1.clone(), e2.clone()]);
    let pub_key = key.with_extension("pub");
    let out = rue(&[
        "journal",
        "verify",
        file.to_str().unwrap(),
        "--key",
        pub_key.to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("every signature verified with"));

    let mut unsigned = e2.clone();
    unsigned.sig = None;
    write_chain(&file, &[e1, unsigned]);
    let out = rue(&["journal", "verify", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "without a key the chain is what is checked"
    );
    let out = rue(&[
        "journal",
        "verify",
        file.to_str().unwrap(),
        "--key",
        pub_key.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unsigned"));
    let _ = fs::remove_dir_all(&d);
}
