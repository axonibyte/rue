//! Gates and proofs over the fake approval binding (5.11): a plan gate
//! holds an instance at Pending until enough proofs arrive; a proof is
//! bound to one request and one scope, so a step's proof opens no other
//! step and none the plan; a wait factor is weight that accrues, so a gate
//! can open with no further proof; a refused proof is journaled and
//! accumulates nothing; a host contract that changes between the request
//! and the approval is R0301 and takes every proof with it; a knell's
//! acknowledgement is a proof in its own scope with a reason.

mod common;

use std::collections::BTreeMap;

use common::world::{self, World, OWNER};
use rue_core::journal::Scope;
use rue_core::model::{
    Ack, Authenticator, Cost, Duration, Factor, GateExpr, Item, PlanGate, Refusal, StepI,
};
use rue_core::states::State;
use rue_engine::gates::FakeApprovalHandle;
use rue_engine::lifecycle::ApplyOptions;

fn opts() -> ApplyOptions {
    ApplyOptions {
        by: "ops".into(),
        ..ApplyOptions::default()
    }
}

fn auths() -> Vec<Authenticator> {
    vec![
        Authenticator {
            id: "oncall".into(),
            human: true,
        },
        Authenticator {
            id: "alice".into(),
            human: true,
        },
    ]
}

fn auth(id: &str) -> Factor {
    Factor::Auth {
        id: id.into(),
        weight: 1,
    }
}

fn two_humans() -> GateExpr {
    GateExpr::Thresh {
        n: 2,
        factors: vec![auth("oncall"), auth("alice")],
    }
}

/// A world whose approval binding is the fake, publishing `auths()`.
fn world(name: &str) -> (World, FakeApprovalHandle) {
    let mut w = World::new(name);
    let a = FakeApprovalHandle::new(auths());
    w.engine.set_approval(Box::new(a.clone()));
    (w, a)
}

#[test]
fn a_plan_gate_holds_the_instance_until_its_proofs_arrive() {
    let (mut w, approval) = world("gate-plan");
    let mut plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan.gate = Some(PlanGate {
        expr: two_humans(),
        window: Some(Duration::new(3_600)),
        allow_zero_human: false,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Pending, "{}", out.line);
    assert_eq!(out.exit, 6, "pending approval is exit 6");
    let id = out.id.clone();
    // One proof is not two.
    let out = w
        .engine
        .approve_proof(&id, Scope::Plan, "oncall", "token-1", "ops")
        .unwrap();
    assert_eq!(out.state, State::Pending, "{}", out.line);
    // The second opens it, and the plan runs.
    let out = w
        .engine
        .approve_proof(&id, Scope::Plan, "alice", "token-2", "sec")
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let events = w.events();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.contains("ProofAccepted"))
            .count(),
        2,
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| e.contains("ProofAccepted") && e.contains("submitter: \"sec\"")),
        "the submitter is the operator, not the authenticator: {events:?}"
    );
    // The binding was asked to verify against the plan scope both times.
    let calls = approval.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.starts_with("verify Plan"))
            .count(),
        2,
        "{calls:?}"
    );
}

#[test]
fn a_proof_binds_to_one_request_and_one_scope() {
    let (mut w, approval) = world("gate-scope");
    let mut plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan.gate = Some(PlanGate {
        expr: two_humans(),
        window: None,
        allow_zero_human: false,
    });
    let out = w
        .engine
        .apply(world::ir(plan.clone()), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    let rec = w.engine.status(&id).unwrap().unwrap();
    let plan_digest = w.engine.scope_digest(&rec, Scope::Plan);
    let step_digest = w.engine.scope_digest(&rec, Scope::Step(1));
    let ack_digest = w.engine.scope_digest(&rec, Scope::Ack(1));
    assert_ne!(plan_digest, step_digest);
    assert_ne!(step_digest, ack_digest);
    assert_ne!(plan_digest, ack_digest);
    // Another request is another nonce, so no proof crosses between them.
    let mut other = world::temp_plan("q", vec![world::step(world::op("b"))]);
    other.gate = plan.gate.clone();
    let out2 = w
        .engine
        .apply(world::ir(other), BTreeMap::new(), opts())
        .unwrap();
    let rec2 = w.engine.status(&out2.id).unwrap().unwrap();
    assert_ne!(
        plan_digest,
        w.engine.scope_digest(&rec2, Scope::Plan),
        "a nonce per request: no proof crosses between them"
    );
    // The challenge the binding rendered is over the scope's digest.
    let _ = w.engine.challenge(&id, Scope::Step(1), "why").unwrap();
    assert!(
        approval
            .calls()
            .iter()
            .any(|c| c.starts_with("challenge Step(1)")),
        "{:?}",
        approval.calls()
    );
}

#[test]
fn a_wait_factor_opens_a_gate_with_no_further_proof() {
    let (mut w, _) = world("gate-wait");
    let mut plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan.gate = Some(PlanGate {
        // One human, or an hour.
        expr: GateExpr::Thresh {
            n: 2,
            factors: vec![
                auth("oncall"),
                Factor::Wait {
                    duration: Duration::new(3_600),
                    weight: 2,
                },
            ],
        },
        // The window must outlast the wait, or the approval window
        // lapses first (fail-closed).
        window: Some(Duration::new(7_200)),
        allow_zero_human: true,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Pending, "{}", out.line);
    let id = out.id.clone();
    // Not yet.
    w.advance(3_599);
    w.engine.reap().unwrap();
    assert_eq!(w.engine.status(&id).unwrap().unwrap().state, State::Pending);
    // The hour is up: the gate opens on the reap pass and the plan runs.
    w.advance(1);
    let report = w.engine.reap().unwrap();
    assert!(
        report.actions.iter().any(|a| a.contains("gate opened")),
        "{:?}",
        report.actions
    );
    assert_eq!(w.engine.status(&id).unwrap().unwrap().state, State::Applied);
}

#[test]
fn a_refused_proof_is_journaled_and_accumulates_nothing() {
    let (mut w, approval) = world("gate-refused");
    approval.refuse("oncall", "the token had expired");
    let mut plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan.gate = Some(PlanGate {
        expr: GateExpr::Single(auth("oncall")),
        window: None,
        allow_zero_human: false,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    let err = w
        .engine
        .approve_proof(&id, Scope::Plan, "oncall", "stale", "ops")
        .unwrap_err()
        .to_string();
    assert!(err.contains("the token had expired"), "{err}");
    let rec = w.engine.status(&id).unwrap().unwrap();
    assert!(rec.proofs.is_empty(), "{:?}", rec.proofs);
    assert_eq!(rec.state, State::Pending);
    assert!(
        w.events()
            .iter()
            .any(|e| e.contains("Denied") && e.contains("the token had expired")),
        "{:?}",
        w.events()
    );
}

#[test]
fn a_host_contract_that_changes_after_the_request_refuses_and_takes_the_proofs() {
    let (mut w, _) = world("gate-r0301");
    let mut plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan.gate = Some(PlanGate {
        expr: two_humans(),
        window: None,
        allow_zero_human: false,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    w.engine
        .approve_proof(&id, Scope::Plan, "oncall", "t", "ops")
        .unwrap();
    assert_eq!(w.engine.status(&id).unwrap().unwrap().proofs.len(), 1);
    // The inventory now says the host runs another OS: the contract the
    // request froze is gone, and with it every proof.
    w.engine.set_hosts(vec![
        {
            let mut h = world::host(OWNER, &["ssh"]);
            h.record.os = "linux".into();
            h
        },
        world::host(world::FAR, &["carrier-pigeon"]),
    ]);
    let out = w
        .engine
        .approve_proof(&id, Scope::Plan, "alice", "t", "ops")
        .unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    let events = w.events();
    assert!(
        events.iter().any(|e| e.contains("HostContractChanged")),
        "{events:?}"
    );
    assert!(events.iter().any(|e| e.contains("R0301")), "{events:?}");
    assert!(w.engine.status(&id).unwrap().unwrap().proofs.is_empty());
}

#[test]
fn a_step_gate_waits_for_the_proof_of_its_own_step() {
    let (mut w, _) = world("gate-step");
    let mut s2 = StepI::new(world::op("b"));
    s2.gate = Some(GateExpr::Single(auth("oncall")));
    let plan = world::temp_plan("p", vec![world::step(world::op("a")), Item::Step(s2)]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Waiting, "{}", out.line);
    assert_eq!(out.exit, 6);
    let id = out.id.clone();
    assert!(
        w.events().iter().any(|e| e.contains("StepGateRequested")),
        "{:?}",
        w.events()
    );
    // A proof in the plan scope is not a proof for this step.
    let out = w
        .engine
        .approve_proof(&id, Scope::Plan, "oncall", "t", "ops")
        .unwrap();
    assert_eq!(out.state, State::Waiting, "{}", out.line);
    // The step's own proof opens it and the walk goes on.
    let out = w
        .engine
        .approve_proof(&id, Scope::Step(2), "oncall", "t", "ops")
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    assert_eq!(w.commands(), vec!["do a", "do b"]);
}

#[test]
fn a_knell_waits_for_its_acknowledgement_and_the_reason_is_journaled() {
    let (mut w, _) = world("gate-ack");
    let mut op = world::op("a");
    // A knell is the point of no return: it has no undo.
    op.undo = rue_core::model::Undo::NoUndo;
    op.refusal = Refusal::Knell {
        guard: None,
        cost: Cost::NoCost("none".into()),
        ack: Ack::Gate(GateExpr::Single(auth("oncall"))),
    };
    let plan = world::temp_plan("p", vec![Item::Knell(StepI::new(op))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Waiting, "{}", out.line);
    let id = out.id.clone();
    assert!(
        w.events().iter().any(|e| e.contains("AckRequested")),
        "{:?}",
        w.events()
    );
    // An acknowledgement without a reason is refused, and so is one whose
    // token the binding will not accept.
    assert!(w.engine.ack(&id, 1, "  ", "t", "oncall").is_err());
    let out = w
        .engine
        .ack(&id, 1, "the customer agreed", "token", "oncall")
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let events = w.events();
    assert!(
        events
            .iter()
            .any(|e| e.contains("KnellAcknowledged") && e.contains("the customer agreed")),
        "{events:?}"
    );
    // The acknowledgement is a proof in its own scope.
    let rec = w.engine.status(&id).unwrap().unwrap();
    assert!(rec.proofs.iter().any(|p| p.scope == Scope::Ack(1)));
}

#[test]
fn an_acknowledgement_the_binding_refuses_is_denied_and_the_knell_stays_shut() {
    let (mut w, approval) = world("gate-ack-refused");
    approval.refuse("oncall", "that token is not yours");
    let mut op = world::op("a");
    op.undo = rue_core::model::Undo::NoUndo;
    op.refusal = Refusal::Knell {
        guard: None,
        cost: Cost::NoCost("none".into()),
        ack: Ack::Gate(GateExpr::Single(auth("oncall"))),
    };
    let plan = world::temp_plan("p", vec![Item::Knell(StepI::new(op))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    let err = w
        .engine
        .ack(&id, 1, "go on then", "stale", "oncall")
        .unwrap_err()
        .to_string();
    assert!(err.contains("that token is not yours"), "{err}");
    assert_eq!(
        w.engine.status(&id).unwrap().unwrap().state,
        State::Waiting,
        "the knell did not fire"
    );
    assert!(
        w.events()
            .iter()
            .any(|e| e.contains("Denied") && e.contains("ack 1")),
        "{:?}",
        w.events()
    );
    assert!(w.commands().is_empty(), "nothing ran");
}

#[test]
fn a_hold_under_auto_mode_is_not_refused_and_reverts_at_wane() {
    // D-086: `:hold` under `mode: :auto` is not refused; a temporary plan
    // reverts it at wane, and holding-then-deciding stays legitimate.
    let (mut w, _) = world("gate-hold-auto");
    let plan = world::temp_plan(
        "p",
        vec![
            world::step(world::hold(world::op("a"))),
            world::step(world::op("b")),
        ],
    );
    let mut opts = opts();
    opts.mode = Some(rue_core::model::Mode::Auto);
    // The second step fails, so the plan refuses with an earlier hold.
    w.ssh.with(|f| {
        f.script.push_back(rue_engine::executor::Scripted::Ok(
            rue_engine::executor::Output::default(),
        ));
        f.script
            .push_back(rue_engine::executor::Scripted::Fail("no".into()));
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts)
        .unwrap();
    assert_eq!(
        out.state,
        State::Held,
        "the hold stands under auto: {}",
        out.line
    );
    assert_eq!(out.exit, 3);
    let id = out.id.clone();
    // Wane reverts it, with no operator in sight.
    w.advance(3_601);
    w.engine.reap().unwrap();
    let rec = w.engine.status(&id).unwrap().unwrap();
    assert_eq!(rec.state, State::Closed, "{rec:?}");
    assert!(
        w.events().iter().any(|e| e.contains("Expired")),
        "{:?}",
        w.events()
    );
}

#[test]
fn an_approval_binding_that_fails_is_r0302_and_opens_nothing() {
    // R0302: a binding that fails at runtime is a refusal naming the
    // binding, not a gate that quietly opens or a panic.
    let (mut w, approval) = world("gate-r0302");
    let mut plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan.gate = Some(PlanGate {
        expr: GateExpr::Single(auth("oncall")),
        window: None,
        allow_zero_human: false,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    approval.with(|f| f.broken = true);
    let err = w
        .engine
        .approve_proof(&id, Scope::Plan, "oncall", "t", "ops")
        .unwrap_err()
        .to_string();
    assert!(err.contains("R0302"), "{err}");
    assert_eq!(
        w.engine.status(&id).unwrap().unwrap().state,
        State::Pending,
        "the gate did not open"
    );
    // The challenge fails the same way, and says which binding.
    let err = w
        .engine
        .challenge(&id, Scope::Plan, "why")
        .unwrap_err()
        .to_string();
    assert!(err.contains("R0302"), "{err}");
}

#[test]
fn a_knell_asks_its_acknowledger_to_accept_the_measured_cost_not_its_name() {
    // What a person acknowledges at a point of no return is the cost, so the
    // cost probe is measured where they are asked. This carried the probe's
    // NAME -- "destroyed_snapshots" where 8.2 asks for a probe "listing what
    // is destroyed" -- and so the one statement a knell exists to put in
    // front of a human was a label.
    let (mut w, _) = world("gate-cost");
    let mut op = world::op("rollback");
    op.undo = rue_core::model::Undo::NoUndo;
    op.refusal = Refusal::Knell {
        guard: None,
        cost: Cost::Probe("destroyed_snapshots".into()),
        ack: Ack::Gate(GateExpr::Single(auth("oncall"))),
    };
    let mut plan = world::temp_plan("p", vec![Item::Knell(StepI::new(op.clone()))]);
    plan.probes.push(world::probe("destroyed_snapshots"));
    w.ssh.observe_as(
        "destroyed_snapshots",
        rue_engine::executor::Observation::yes("tank/rue/a@late, tank/rue/b@late"),
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Waiting, "{}", out.line);
    let asked = w
        .events()
        .into_iter()
        .find(|e| e.contains("AckRequested"))
        .expect("an acknowledgement is requested");
    assert!(
        asked.contains("tank/rue/a@late, tank/rue/b@late"),
        "the acknowledger was shown a label, not what is destroyed: {asked}"
    );

    // A cost that cannot be measured refuses the step: nobody is asked to
    // accept a point of no return blind.
    // A different fact, so this instance does not wait on the first one's
    // reservation: that one is still waiting for its acknowledgement.
    op.footprint = vec![rue_core::model::FootprintEntry::entry(
        rue_core::model::Kind::Owned,
        "file:/rollback-q",
    )];
    let mut plan = world::temp_plan("q", vec![Item::Knell(StepI::new(op))]);
    plan.probes.push(world::probe("unmeasurable"));
    if let Some(Item::Knell(s)) = plan.body.first_mut() {
        s.op.refusal = Refusal::Knell {
            guard: None,
            cost: Cost::Probe("unmeasurable".into()),
            ack: Ack::Gate(GateExpr::Single(auth("oncall"))),
        };
    }
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert!(
        !w.events()
            .iter()
            .any(|e| e.contains("AckRequested") && e.contains("unmeasurable")),
        "an unmeasured cost was put in front of the acknowledger anyway"
    );
}

#[test]
fn a_static_probe_is_frozen_where_it_runs_and_a_write_before_the_ack_refuses_it() {
    // 8.2's failback guard, "measured twice": written bytes since the split,
    // frozen when the request is made and measured again when the
    // acknowledgement arrives, so a write in between invalidates the ack
    // rather than being rolled back over.
    //
    // The contract observed every static probe on every touched host with
    // an empty body and dropped whatever failed. This probe runs on the
    // controller, so it was asked of the wrong executor, failed, and was
    // silently left out -- and no change to it could ever be noticed.
    let (mut w, _) = world("gate-static");
    let mut op = world::op("rollback");
    op.undo = rue_core::model::Undo::NoUndo;
    op.refusal = Refusal::Knell {
        guard: None,
        cost: Cost::NoCost("measured by the preflight".into()),
        ack: Ack::Gate(GateExpr::Single(auth("oncall"))),
    };
    let mut plan = world::temp_plan("p", vec![Item::Knell(StepI::new(op))]);
    let mut written = world::probe("written_bytes_since_split");
    written.locus = rue_core::model::Locus::Controller;
    written.static_ = true;
    plan.probes.push(written);
    w.local.observe_as(
        "written_bytes_since_split",
        rue_engine::executor::Observation::yes("0"),
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Waiting, "{}", out.line);
    let id = out.id.clone();

    // Somebody writes to the dataset while the acknowledgement is pending.
    w.local.observe_as(
        "written_bytes_since_split",
        rue_engine::executor::Observation::yes("4096"),
    );
    let out = w
        .engine
        .ack(&id, 1, "fail back now", "token", "oncall")
        .unwrap();
    assert_ne!(
        out.state,
        State::Applied,
        "the rollback went ahead over a write made after the request was measured"
    );
    assert!(
        w.events().iter().any(|e| e.contains("HostContractChanged")),
        "the second measurement disagreed with the first and nothing said so: {:?}",
        w.events()
    );
}

#[test]
fn a_static_probe_that_moves_while_a_step_gate_waits_refuses_rather_than_crashing() {
    // The same freeze, reached through a step gate's proof while the
    // instance is Waiting. The contract check called the Pending transition
    // in every state, and the state machine has none for Waiting or
    // Applying: this ended in WrongState. A Waiting instance now leaves the
    // judgement to walk, which re-derives the contract before any step runs.
    let (mut w, _) = world("gate-static-step");
    let mut plan = world::temp_plan(
        "p",
        vec![Item::Step(StepI {
            gate: Some(GateExpr::Single(auth("oncall"))),
            ..StepI::new(world::op("a"))
        })],
    );
    let mut written = world::probe("written_bytes_since_split");
    written.locus = rue_core::model::Locus::Controller;
    written.static_ = true;
    plan.probes.push(written);
    w.local.observe_as(
        "written_bytes_since_split",
        rue_engine::executor::Observation::yes("0"),
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Waiting, "{}", out.line);
    w.local.observe_as(
        "written_bytes_since_split",
        rue_engine::executor::Observation::yes("4096"),
    );
    let out = w
        .engine
        .approve_proof(&out.id, Scope::Step(1), "oncall", "token", "ops")
        .expect("a contract change mid-plan is a refusal, never an error");
    assert_ne!(out.state, State::Applied, "{}", out.line);
    assert!(
        w.commands().is_empty(),
        "a step ran on a contract that changed since the request: {:?}",
        w.commands()
    );
}
