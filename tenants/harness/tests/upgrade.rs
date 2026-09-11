//! Upgrade vectors (ROADMAP Phase 5; docs/issues/0001): a release's tenant
//! texts, as that release shipped them (tenants/_upgrade/<release>/), under
//! the current build. Each must check clean or be refused only by codes
//! added after its release -- codes that say when they came and what to
//! change -- because "the text a person wrote for the last release no
//! longer checks, and nothing says why" is the failure this exists to
//! prevent.

use std::path::{Path, PathBuf};

use rue_core::check::check;
use rue_core::diagnostics::Code;
use rue_tenants::golden::repo_root;

/// A release's tenant cases, as its own `TENANT_CASES` listed them:
/// (tenant, owner host, plan, requester). v0.1.0 and v0.2.0 list the same.
const CASES: &[(&str, &str, &str, &str)] = &[
    ("t1", "db-01", "breakglass", "ops_requester"),
    ("t2", "node-b", "promote_auto", "operator"),
    ("t2", "node-b", "promote", "operator"),
    ("t3", "fw-01", "open_mgmt_port", "netops_requester"),
    ("t3", "fw-win-01", "open_mgmt_port", "netops_requester"),
    ("t3", "fw-02", "open_mgmt_port", "netops_requester"),
    ("t3", "fw-win-02", "open_mgmt_port", "netops_requester"),
    ("t3", "fw-lnx-01", "open_mgmt_port", "netops_requester"),
    ("t3", "fw-mac-01", "open_mgmt_port", "netops_requester"),
    ("t4", "site-ctl", "shed_load", "reactive_host"),
    ("t4", "site-ctl", "shed_load_deferring", "reactive_host"),
];

const RELEASES: &[&str] = &["v0.1.0", "v0.2.0"];

/// `vX.Y.Z` as numbers, for ordering releases.
fn version(v: &str) -> (u32, u32, u32) {
    let n: Vec<u32> = v
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse().unwrap())
        .collect();
    (n[0], n[1], n[2])
}

/// How a release's user checked a text: `--inventory` arrived with E0607
/// in v0.2.0, so a v0.1.0 text is checked as v0.1.0 checked it, without,
/// and a hook inventory's E0607 is then the migration it should be told.
fn check_as(
    release: &str,
    text: &Path,
    host: &str,
    plan: &str,
    requester: &str,
) -> Vec<(Code, String)> {
    let inventory =
        (version(release) >= (0, 2, 0)).then(|| text.parent().unwrap().join("inventory.toml"));
    let opts = rue_surface::resolve::Options {
        suspend_e0604: false,
        host: Some(host.to_string()),
        plan: Some(plan.to_string()),
        requester: Some(requester.to_string()),
        inventory,
    };
    match rue_surface::resolve::resolve(text, &opts) {
        Err(diags) => diags.into_iter().map(|d| (d.code, d.message)).collect(),
        Ok(ir) => check(&ir.site, &ir.requester, &ir.plan)
            .diagnostics
            .into_iter()
            .map(|d| (d.code, d.message))
            .collect(),
    }
}

fn texts_of(release: &str) -> PathBuf {
    repo_root()
        .expect("the repository root")
        .join("tenants/_upgrade")
        .join(release)
}

#[test]
fn a_released_text_checks_clean_or_is_told_what_changed_since() {
    let mut failures = Vec::new();
    let mut refused = 0;
    for release in RELEASES {
        for (tenant, host, plan, requester) in CASES {
            let text = texts_of(release).join(tenant).join("plan.rue");
            assert!(text.is_file(), "{} is missing", text.display());
            for (code, message) in check_as(release, &text, host, plan, requester) {
                refused += 1;
                let newer = code.since().is_some_and(|v| version(v) > version(release));
                if !newer || !message.contains("(new in v") {
                    failures.push(format!(
                        "{release} {tenant} {plan} on {host}: {code} {message}\n    \
                         -- not a rule added after {release}, or not saying so"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // The vectors are only worth having while they catch something: the
    // first release's T1 and T4 and both releases' Windows T3 meet rules
    // added since. A day this is zero, a newer release directory is due.
    assert!(refused > 0, "no released text meets a newer rule");
}
