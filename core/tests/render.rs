//! The prose and explain renderers at unit level: the clauses whose exact
//! bytes the goldens depend on, stated on small verdicts and plans. The
//! tenants' goldens are the full proof; these pin the rules a golden would
//! only show indirectly.

mod common;

use common::*;
use rue_core::check::check;
use rue_core::explain::explain;
use rue_core::model::*;
use rue_core::prose::prose;

fn site() -> Site {
    Site {
        hosts: vec![HostRecord {
            name: "db-01".into(),
            os: "freebsd".into(),
            reach: vec!["ssh".into()],
            filesystem: true,
        }],
        transports: vec!["ssh".into()],
        authenticators: vec![Authenticator {
            id: "oncall".into(),
            human: true,
        }],
        max_wait: None,
        scheduler_present: vec!["db-01".into()],
    }
}

#[test]
fn a_clean_temporary_plan_reads_as_the_grammar_says() {
    let p = temp(vec![s(owned("a")), s(owned("b"))]);
    assert_eq!(
        prose(&check(&site(), "requester", &p)),
        "p on db-01: temporary; reverts at wane 1h; fully reversible (2 steps); steps 1, 2 revert only while the engine lives; clause dispatch from inventory.\n"
    );
}

#[test]
fn a_refused_plan_leads_with_its_diagnostics() {
    let p = temp(vec![s(owned("a")), s(owned("a"))]);
    let text = prose(&check(&site(), "requester", &p));
    assert!(
        text.starts_with("p on db-01: refused; E0301 at step 2: steps 1 and 2 both write file:/a and step 1's undo needs it.\n"),
        "{text}"
    );
}

#[test]
fn a_backstop_range_uses_an_en_dash_and_late_arming_is_stated() {
    let p = Plan {
        backstop: Some(Backstop {
            triggers: vec![Trigger::After(Duration::new(3600))],
            arm_before: 3,
        }),
        ..temp(vec![
            s(Op {
                undo_locus: UndoLocus::Target,
                ..owned("a")
            }),
            s(Op {
                undo_locus: UndoLocus::Target,
                ..owned("b")
            }),
        ])
    };
    let text = prose(&check(&site(), "requester", &p));
    let expected = "expiry backstop (after 1h) covers steps 1\u{2013}2 on the target, installed before step 1, armed after step 2, engine-only for steps 1\u{2013}2 until armed, fires within ~1m after the deadline, self-enforced on db-01, drift: 1 clobber, 2 clobber, snapshots on target (cap 1048576)";
    assert!(text.contains(expected), "{text}");
}

#[test]
fn explain_numbers_leaves_joins_fields_with_three_spaces_and_redacts_every_secret() {
    let mut o = owned("a");
    o.outputs = vec![
        Output {
            name: "pw".into(),
            secret: true,
        },
        Output {
            name: "port".into(),
            secret: false,
        },
        Output {
            name: "key".into(),
            secret: true,
        },
    ];
    o.undo_one_line = "revoke key, clear pw, keep port".into();
    let p = Plan::new(
        "p",
        "db-01",
        vec![
            Item::Step(StepI {
                args: vec!["x: 1".into()],
                ..StepI::new(o)
            }),
            Item::Commit,
        ],
    );
    // Redaction is a textual replace of each secret output's name, applied
    // last-declared first as the prototype folds; an output whose name is a
    // substring of another's would be garbled either way, a Phase 0 quirk the
    // goldens never reach and the surface's name rules will make moot.
    let expected = " 1. a(x: 1)   locus=target   refusal=revert   drift=clobber   undo=revoke <secret:key>, clear <secret:pw>, keep port   undo_locus=controller\n 2. commit()   ends the plan: undo discarded, umbras released, backstops disarmed\n";
    assert_eq!(explain(&p, &[]), expected);
}

#[test]
fn explain_marks_knells_deferred_steps_and_the_region_cost() {
    let region = Op {
        undo_locus: UndoLocus::Target,
        ..Op::new(
            "pf",
            vec![FootprintEntry::anchored("file:/etc/pf.conf", "rue")],
        )
    };
    let p = temp(vec![Item::Knell(StepI::new(knell_op())), s(region)]);
    let expected = " 1. fence   locus=target   refusal=knell   drift=n/a   undo=NO UNDO \u{2014} knell, cost fence_verdict   undo_locus=controller   ack=none (driver verified off)\n 2. pf   locus=target   refusal=revert   drift=clobber   undo=restore   undo_locus=target   damaged-marker cost: the whole fact is restored from the do-time snapshot and a stranger's edits outside the region are lost, unless another instance holds a region on it   deferred \u{2192} (handoff command printed at apply)\n";
    assert_eq!(explain(&p, &[2]), expected);
}
