//! Tier 4 through the driver: every applicable row of the transition table
//! `rue_core::states::transition_table()` generates is seeded as an
//! instance in the store and its event fired through the engine's public
//! surface (a verb, `advance`, the reap pass, or boot). The transition the
//! engine records must be the row's outcome. Rows this unit cannot reach
//! are named, not skipped: the set of undriven events is asserted exactly,
//! so a later unit that makes one reachable has to remove it here.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::world::{self, World, FAR, OWNER, T0};
use rue_core::intent::Intent;
use rue_core::model::{
    Duration, FootprintEntry, ForceName, Instant, Item, Kind, Mode, OnLapse, StepI, Tri,
};
use rue_core::states::{transition_table, Ctx, Event as E, Outcome, RCode, State, ALL_EVENTS};
use rue_engine::executor::{Observation, Output, Scripted};
use rue_engine::lifecycle::{
    AppliedStep, ApplyOptions, DeferredAt, EngineError, InstanceRecord, Wait,
};

/// Events no verb, walk, reap or boot of this unit can produce, with the
/// unit that brings each.
const UNDRIVEN: &[(E, &str)] = &[
    (
        E::HostContractChanged,
        "unit E: the host contract re-derived at request, approval and apply",
    ),
    (E::DriftOnDefer, "unit C: undo-time drift under :defer"),
];

fn plan_for(ctx: Ctx, ev: E) -> rue_core::model::Plan {
    let mut a = world::op("a");
    if ctx.earlier_hold {
        a = world::hold(a);
    }
    let mut b = world::op("b");
    match ev {
        E::WaitAtStep | E::WaitSatisfied | E::BoundLapses => {
            b.pre = vec![world::guard("g", Tri::Unknown)];
        }
        E::DeferAtStep | E::HandoffDone => {
            b = world::on(b, FAR);
            b.handoff_done = Some("far_done".into());
        }
        E::Suspend | E::Reestablish => {
            a.footprint = vec![FootprintEntry::entry(Kind::Held, "proc:tunnel")];
            a.suspend = Some(world::run("suspend"));
            a.reestablish = Some(world::run("resume"));
        }
        _ => {}
    }
    let mut sb = StepI::new(b);
    sb.on_lapse = ctx.on_lapse;
    let mut body = vec![Item::Step(StepI::new(a)), Item::Step(sb)];
    if ev == E::CommitItem && ctx.intent == Intent::Temporary {
        // A commit() item in a temporary plan: the checker refuses it
        // (E0501); seeded past the check, the machine refuses it (R0102).
        body.push(Item::Commit);
    }
    let mut p = match ctx.intent {
        Intent::Temporary => world::temp_plan("p", body),
        Intent::Permanent => world::perm_plan("p", body),
    };
    p.mode = ctx.mode;
    if matches!(ev, E::ApprovalWindowLapses) || matches!(ev, E::Cancel) {
        p.gate = Some(rue_core::model::PlanGate {
            expr: rue_core::model::GateExpr::Single(rue_core::model::Factor::Auth {
                id: "oncall".into(),
                weight: 1,
            }),
            window: Some(Duration::new(1800)),
            allow_zero_human: false,
        });
    }
    p
}

fn applied(steps: &[u32]) -> Vec<AppliedStep> {
    steps
        .iter()
        .map(|s| AppliedStep {
            step: *s,
            iteration: 0,
        })
        .collect()
}

/// An instance in `state`, consistent enough for the engine to act on it.
fn seed(ctx: Ctx, state: State, ev: E) -> InstanceRecord {
    let plan = plan_for(ctx, ev);
    let permanent = ctx.intent == Intent::Permanent;
    let now = Instant::new(T0);
    let mut rec = InstanceRecord {
        id: "seeded".into(),
        ir: world::ir(plan),
        params: BTreeMap::new(),
        state,
        permanent,
        rehearsal: false,
        requested_at: Some(now),
        approved_at: Some(now),
        deadline: (!permanent).then(|| Instant::new(T0 + 3600)),
        approval_deadline: None,
        applied: Vec::new(),
        choices: BTreeMap::new(),
        outputs: BTreeMap::new(),
        snapshots: BTreeMap::new(),
        waiting: None,
        held_at: None,
        deferred: None,
        stuck: Vec::new(),
        drift_held: Vec::new(),
        acks: Vec::new(),
        forced: Vec::new(),
        ledger_ids: Vec::new(),
        refusal: None,
        closed_reason: None,
    };
    match state {
        State::Pending => {
            rec.approved_at = None;
            rec.deadline = None;
            rec.approval_deadline = Some(Instant::new(T0 + 1800));
        }
        State::ApprovalExpired => {
            rec.approved_at = None;
            rec.deadline = None;
        }
        State::Applying => rec.applied = applied(&[1]),
        State::Waiting => {
            rec.applied = applied(&[1]);
            rec.waiting = Some(Wait {
                step: 2,
                reason: "unknown guard g".into(),
                since: now,
                bound: Some(Instant::new(T0 + 600)),
                guard: Some("g".into()),
            });
        }
        State::Deferred => {
            rec.applied = applied(&[1]);
            rec.deferred = Some(DeferredAt {
                step: 2,
                handoff: "far_done".into(),
            });
        }
        State::Held => {
            rec.applied = applied(&[1]);
            rec.held_at = Some(2);
        }
        State::Applied | State::Suspended | State::Expired | State::Reverting => {
            rec.applied = applied(&[1, 2]);
        }
        State::Stuck => {
            rec.applied = applied(&[1]);
            rec.stuck = vec![1];
        }
        State::DriftHeld => {
            rec.applied = applied(&[1]);
            rec.drift_held = vec![1];
        }
        State::Unchecked | State::Checked | State::Committed | State::Closed => {}
    }
    rec
}

/// Fire the event; the result is the verb's error when it refused.
fn fire(w: &mut World, ctx: Ctx, state: State, ev: E) -> Result<(), EngineError> {
    let id = "seeded";
    let r: Result<_, EngineError> = match ev {
        E::Check | E::Request | E::Approve => {
            // These three happen inside apply, on a fresh plan.
            let plan = plan_for(ctx, ev);
            w.engine
                .apply(
                    world::ir(plan),
                    BTreeMap::new(),
                    ApplyOptions {
                        by: "ops".into(),
                        ..ApplyOptions::default()
                    },
                )
                .map(|_| ())
        }
        E::ApprovalWindowLapses => {
            w.advance(1800);
            w.engine.reap().map(|_| ())
        }
        E::Cancel => match state {
            State::Pending => w.engine.cancel(id).map(|_| ()),
            _ => w.engine.reap().map(|_| ()),
        },
        E::AllStepsDone | E::CommitItem | E::WaitAtStep | E::DeferAtStep => {
            w.ssh.observe_as("g", Observation::unknown("?"));
            w.engine.advance(id).map(|_| ())
        }
        E::Refuse => {
            w.ssh.script(vec![Scripted::Fail("boom".into())]);
            w.engine.advance(id).map(|_| ())
        }
        E::WaitSatisfied => {
            w.ssh.observe_as("g", Observation::yes("ok"));
            w.engine.reap().map(|_| ())
        }
        E::HandoffDone => w.engine.handoff_done(id, 2, "ops").map(|_| ()),
        E::Recant => w.engine.recant(id, &[]).map(|_| ()),
        E::Suspend => {
            w.ssh.script(vec![Scripted::Fail("gone".into())]);
            w.engine.boot().map(|_| ())
        }
        E::Reestablish => w.engine.boot().map(|_| ()),
        E::Renew => {
            w.advance(3000);
            w.engine.renew(id, Duration::new(3600)).map(|_| ())
        }
        E::Confirm => w.engine.confirm(id).map(|_| ()),
        E::CommitVerb => w.engine.commit(id, "ops", "done").map(|_| ()),
        E::Resume => w.engine.resume(id, "ops").map(|_| ()),
        E::BoundLapses => {
            w.advance(600);
            w.engine.reap().map(|_| ())
        }
        E::WaneElapses => {
            w.advance(3600);
            w.engine.reap().map(|_| ())
        }
        E::UndoClean => w.engine.reap().map(|_| ()),
        E::UndoFailed => {
            w.ssh.script(vec![Scripted::Fail("undo broke".into())]);
            w.engine.reap().map(|_| ())
        }
        E::Retry => w.engine.reap().map(|_| ()),
        E::ForceDrift => w.engine.recant(id, &[ForceName::Drift]).map(|_| ()),
        E::Abandon => w.engine.abandon(id, "admin", "why").map(|_| ()),
        E::HostContractChanged | E::DriftOnDefer => unreachable!("undriven"),
    };
    r
}

#[test]
fn every_applicable_row_of_the_transition_table_is_driven_through_the_engine() {
    let undriven: BTreeSet<E> = UNDRIVEN.iter().map(|(e, _)| *e).collect();
    let mut driven = 0usize;
    let mut skipped: BTreeSet<E> = BTreeSet::new();
    let mut failures = Vec::new();
    for (ctx, state, ev, expected) in transition_table() {
        if undriven.contains(&ev) {
            skipped.insert(ev);
            continue;
        }
        let mut w = World::new("table");
        let fresh = matches!(ev, E::Check | E::Request | E::Approve);
        if !fresh {
            let rec = seed(ctx, state, ev);
            w.engine.store().write_instance(&rec.id, &rec).unwrap();
        }
        let result = fire(&mut w, ctx, state, ev);
        let label = format!(
            "{:?}/{:?}/hold={}/lapse={:?}: {state} --{ev}--> expected {expected:?}",
            ctx.intent, ctx.mode, ctx.earlier_hold, ctx.on_lapse
        );
        match expected {
            Outcome::Refuse(code) => match result {
                Err(EngineError::NotAdmitted(c, _)) if c == code => {}
                other => failures.push(format!("{label}; got {other:?}")),
            },
            Outcome::To(_) | Outcome::Stay => {
                if let Err(e) = &result {
                    failures.push(format!("{label}; the driver failed: {e}"));
                    continue;
                }
                let found = w
                    .engine
                    .trace()
                    .iter()
                    .find(|t| t.from == state && t.event == ev)
                    .cloned();
                match found {
                    Some(t) if t.outcome == expected => {}
                    Some(t) => failures.push(format!("{label}; got {:?}", t.outcome)),
                    None => failures.push(format!(
                        "{label}; no such transition in the trace: {:?}",
                        w.engine
                            .trace()
                            .iter()
                            .map(|t| format!("{}--{}-->{:?}", t.from, t.event, t.outcome))
                            .collect::<Vec<_>>()
                    )),
                }
            }
            Outcome::NotApplicable => unreachable!("the table lists applicable rows only"),
        }
        driven += 1;
    }
    assert!(
        failures.is_empty(),
        "{} rows failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(
        skipped, undriven,
        "the undriven set is exactly the declared one"
    );
    // Every event either drives or is declared, and the declared ones are
    // absent from the driven rows.
    for e in ALL_EVENTS {
        assert!(
            undriven.contains(e) || transition_table().iter().any(|(_, _, ev, _)| ev == e),
            "{e} is neither driven nor declared"
        );
    }
    assert!(driven >= 600, "{driven} rows driven");
    let _ = (Mode::Manual, OnLapse::Revert, OWNER, Output::default());
}

#[test]
fn the_undriven_rows_are_named_with_the_unit_that_brings_them() {
    for (e, why) in UNDRIVEN {
        assert!(why.starts_with("unit "), "{e}: {why}");
    }
    assert_eq!(
        UNDRIVEN.len(),
        2,
        "a new undriven event needs its unit named here"
    );
    let _ = RCode::R0102;
}
