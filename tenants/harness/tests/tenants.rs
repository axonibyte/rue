//! Tier 2, the Phase 0 acceptance (docs/ROADMAP.md section 9) held by the
//! Rust crates: every tenant checks clean; every negative refuses with
//! exactly its code; the negative goldens cover exactly the codes the checker
//! emits; and the claims section 8 makes about each verdict hold as fields.
//! Also the source-as-data half: every case directory carries its `.rue`
//! text and every tenant an inventory.

use std::collections::BTreeSet;
use std::path::Path;

use rue_core::check::check;
use rue_core::diagnostics::Code;
use rue_core::intent::Intent;
use rue_core::model::{Duration, Strictness};
use rue_core::verdict::{HostTouched, Status, Verdict};
use rue_tenants::golden::repo_root;
use rue_tenants::{load, TenantCase, EMITTED_CODES, NEGATIVES, TENANT_CASES};

fn verdict(root: &Path, input: &str) -> (Verdict, rue_core::ir::PlanIr) {
    let ir = load(root, input).unwrap_or_else(|e| panic!("{e}"));
    (check(&ir.site, &ir.requester, &ir.plan), ir)
}

fn case(root: &Path, tenant: &str, host: &str) -> Verdict {
    verdict(
        root,
        &TenantCase {
            tenant: Box::leak(tenant.to_string().into_boxed_str()),
            host: Box::leak(host.to_string().into_boxed_str()),
        }
        .input(),
    )
    .0
}

#[test]
fn every_tenant_checks_clean() {
    let root = repo_root().unwrap();
    for c in TENANT_CASES {
        let (v, _) = verdict(&root, &c.input());
        assert_eq!(v.diagnostics, vec![], "{}", c.dir());
        assert_eq!(v.status, Status::Ok, "{}", c.dir());
    }
}

#[test]
fn every_negative_refuses_with_exactly_its_code() {
    let root = repo_root().unwrap();
    for n in NEGATIVES {
        let (v, _) = verdict(&root, &n.input());
        assert_eq!(v.status, Status::Refused, "{}", n.name());
        let codes: Vec<Code> = v.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(codes, vec![n.code], "{}", n.name());
    }
}

#[test]
fn the_negative_goldens_cover_exactly_the_emitted_codes() {
    let covered: BTreeSet<Code> = NEGATIVES.iter().map(|n| n.code).collect();
    let emitted: BTreeSet<Code> = EMITTED_CODES.iter().copied().collect();
    assert_eq!(covered, emitted);
    let mut sorted = EMITTED_CODES.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted, EMITTED_CODES,
        "the emitted list is sorted and duplicate-free"
    );
}

#[test]
fn case_directories_are_distinct() {
    let names: BTreeSet<String> = NEGATIVES.iter().map(|n| n.name()).collect();
    assert_eq!(names.len(), NEGATIVES.len());
    let cases: BTreeSet<String> = TENANT_CASES.iter().map(|c| c.dir()).collect();
    assert_eq!(cases.len(), TENANT_CASES.len());
}

#[test]
fn the_rue_text_and_inventory_exist_for_every_case() {
    let root = repo_root().unwrap();
    let tenants: BTreeSet<&str> = TENANT_CASES.iter().map(|c| c.tenant).collect();
    for t in tenants {
        for f in ["plan.rue", "inventory.toml"] {
            let p = root.join("tenants").join(t).join(f);
            assert!(p.is_file(), "missing {}", p.display());
        }
    }
    for n in NEGATIVES {
        let p = root
            .join("tenants")
            .join("_negative")
            .join(n.name())
            .join("plan.rue");
        assert!(p.is_file(), "missing {}", p.display());
    }
}

#[test]
fn t1_break_glass_section_8_1() {
    let root = repo_root().unwrap();
    let t1 = case(&root, "t1", "db-01");
    assert_eq!(t1.intent, Some(Intent::Temporary));
    assert_eq!(t1.wane, Some(Duration::new(14_400)));
    assert_eq!(t1.reversible_through, 4);
    assert_eq!(t1.point_of_no_return, None);
    let b = t1.backstop.as_ref().unwrap();
    assert_eq!(b.covers, vec![1, 2]);
    assert_eq!(b.installed_before, Some(1));
    assert_eq!(b.armed_after, Some(2));
    assert_eq!(b.late_arming_window, vec![1, 2]);
    assert_eq!(t1.controller_only_undos, vec![3, 4]);
    assert_eq!(
        t1.hosts_touched
            .iter()
            .find(|(n, _)| *n == 3)
            .map(|(_, h)| h.clone()),
        Some(vec![HostTouched::Host {
            host: "bmc-01".into(),
            directory: "controller".into()
        }])
    );
    let conditionals: Vec<Option<String>> =
        t1.steps.iter().map(|s| s.conditional.clone()).collect();
    assert_eq!(
        conditionals,
        vec![
            None,
            Some("foreign region in file:/root/.ssh/authorized_keys".into()),
            None,
            None
        ]
    );
    assert_eq!(
        b.conditional.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        vec![2]
    );
    let g = t1.gate.as_ref().unwrap();
    assert!(g.satisfiable);
    assert_eq!(g.min_distinct_humans, Some(1));
    assert_eq!(g.window, Some(Duration::new(1800)));
}

#[test]
fn t2_succession_section_8_2() {
    let root = repo_root().unwrap();
    let (auto, ir_auto) = verdict(
        &root,
        &TenantCase {
            tenant: "t2",
            host: "node-b-auto",
        }
        .input(),
    );
    let manual = case(&root, "t2", "node-b-manual");
    assert_eq!(auto.intent, Some(Intent::Permanent));
    assert_eq!(auto.commit_step, Some(9));
    assert_eq!(manual.commit_step, Some(10));
    assert_eq!(auto.reversible_through, 3);
    let p = auto.point_of_no_return.as_ref().unwrap();
    assert_eq!(
        (p.step, p.guard.as_deref(), p.cost.as_str(), p.ack.as_str()),
        (4, Some("fence_verified_off"), "fence_verdict", "none")
    );
    assert_eq!(
        manual.point_of_no_return.as_ref().map(|p| p.ack.as_str()),
        Some("humans()")
    );
    assert!(!auto.holds_at.is_empty());
    assert_eq!(auto.held_indefinitely, vec![5, 6, 8]);
    assert_eq!(manual.held_indefinitely, vec![6, 7, 9]);
    assert_eq!(auto.deferred, vec![8]);
    assert_eq!(manual.deferred, vec![9]);
    assert_eq!(
        auto.steps.iter().filter(|s| s.refusal == "knell").count(),
        1
    );
    assert_eq!(
        manual.steps.iter().filter(|s| s.refusal == "knell").count(),
        2
    );
    assert_eq!(
        ir_auto.plan.strictness,
        Strictness::Strict,
        "the per-guest loop checks clean under strict"
    );
    assert!(auto.may_conflicts.is_empty());
    assert_eq!(auto.mode, "auto");
    assert_eq!(manual.mode, "manual");
    assert_eq!(auto.gate, None, "the auto plan contains no human wait");
    assert!(auto.steps.iter().all(|s| s.gate.is_none()));
    assert_eq!(auto.induced_defer, vec![5, 6, 8]);
}

#[test]
fn t3_commit_confirmed_change_section_8_3() {
    let root = repo_root().unwrap();
    let pf = case(&root, "t3", "fw-01");
    let win = case(&root, "t3", "fw-win-01");
    for v in [&pf, &win] {
        assert_eq!(v.intent, Some(Intent::Permanent));
        assert_eq!(v.commit_step, Some(4));
        assert_eq!(v.reversible_through, 1);
        let b = v.backstop.as_ref().unwrap();
        assert_eq!(b.covers, vec![1]);
        assert_eq!(b.installed_before, Some(1));
        assert_eq!(b.armed_before, Some(1));
        assert_eq!(b.armed_after, None);
    }
    let conditionals = |v: &Verdict| {
        v.steps
            .iter()
            .map(|s| s.conditional.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        conditionals(&pf),
        vec![
            Some("foreign region in file:/etc/pf.conf".into()),
            None,
            None,
            None
        ]
    );
    assert_eq!(conditionals(&win), vec![None, None, None, None]);
}

#[test]
fn t4_reactive_host_section_8_4() {
    let root = repo_root().unwrap();
    let clobber = case(&root, "t4", "site-ctl");
    let defer = case(&root, "t4", "site-ctl-defer");
    for v in [&clobber, &defer] {
        assert_eq!(v.intent, Some(Intent::Temporary));
        assert_eq!(v.wane, Some(Duration::new(7200)));
        assert_eq!(v.reversible_through, 1);
        assert_eq!(v.backstop, None);
    }
    assert_eq!(
        clobber
            .steps
            .iter()
            .map(|s| s.drift.clone())
            .collect::<Vec<_>>(),
        vec![Some("clobber".into())]
    );
    assert_eq!(
        defer
            .steps
            .iter()
            .map(|s| s.drift.clone())
            .collect::<Vec<_>>(),
        vec![Some("defer".into())]
    );
    assert_eq!(
        clobber
            .hosts_touched
            .iter()
            .find(|(n, _)| *n == 1)
            .map(|(_, h)| h.clone()),
        Some(vec![HostTouched::Host {
            host: "site-ctl".into(),
            directory: "controller".into()
        }])
    );
    assert_eq!(clobber.controller_only_undos, vec![1]);
}
