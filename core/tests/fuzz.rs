//! Tier 4 fuzz (docs/ROADMAP.md Phase 1 task 10): over seeded random sites
//! and plans, `check` never panics and its verdict is canonical; the prose
//! and the listing never panic; the plan IR round-trips. `RUE_FUZZ_SEED` and
//! `RUE_FUZZ_STEPS` override the defaults; a failure names the seed and step.

mod common;

use common::gen::*;
use rue_core::check::{check, deferred_steps};
use rue_core::explain::explain;
use rue_core::ir::{parse, PlanIr, IR_VERSION};
use rue_core::json::canonical;
use rue_core::prose::prose;
use rue_core::verdict::to_json;

const STEPS: u32 = 500;

#[test]
fn check_never_panics_and_its_verdict_is_canonical() {
    each_step(0x5EED_0001, STEPS, |rng, _| {
        let site = gen_site(rng);
        let plan = gen_plan(rng, &site);
        let requester = gen_requester(rng, &site);
        let v = check(&site, &requester, &plan);
        let json = to_json(&v);
        let bytes = canonical::encode(&json).expect("a verdict encodes canonically");
        let back: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            back, json,
            "the verdict's JSON survives its canonical bytes"
        );
        assert_eq!(canonical::encode(&back).unwrap(), bytes);
    });
}

#[test]
fn prose_and_explain_never_panic() {
    each_step(0x5EED_0002, STEPS, |rng, _| {
        let site = gen_site(rng);
        let plan = gen_plan(rng, &site);
        let requester = gen_requester(rng, &site);
        let v = check(&site, &requester, &plan);
        let p = prose(&v);
        assert!(p.ends_with('\n') && !p.trim().is_empty());
        let d = deferred_steps(&site, &plan);
        let e = explain(&plan, &d);
        assert!(e.is_empty() || e.ends_with('\n'));
        // The page renders whatever the listing does, and closes every tag
        // it opens however strange the plan is.
        let h = rue_core::explain::explain_html(&plan, &d, Some(&p));
        assert!(h.starts_with("<!DOCTYPE html>") && h.trim_end().ends_with("</html>"));
        assert_eq!(
            h.matches("<tr").count(),
            h.matches("</tr>").count(),
            "every row is closed"
        );
        assert!(
            !h.contains("<script"),
            "nothing the plan carries becomes a script"
        );
    });
}

#[test]
fn the_plan_ir_round_trips() {
    each_step(0x5EED_0003, STEPS, |rng, _| {
        let site = gen_site(rng);
        let plan = gen_plan(rng, &site);
        let ir = PlanIr {
            ir_version: IR_VERSION,
            requester: gen_requester(rng, &site),
            site,
            plan,
        };
        let bytes = canonical::encode(&serde_json::to_value(&ir).unwrap()).unwrap();
        let back = parse(&bytes).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(back, ir);
        assert_eq!(
            canonical::encode(&serde_json::to_value(&back).unwrap()).unwrap(),
            bytes
        );
    });
}
