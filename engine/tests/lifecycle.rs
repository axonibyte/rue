//! The lifecycle over fakes: apply, refuse and revert, stuck and retry,
//! silence, wane, renewal, commit, a gate, an unknown guard, a hold, a
//! deferred step, a restore undo, a rehearsal, exclusivity, boot recovery
//! and settle. Every scenario reads the journal the sink received and the
//! commands the fake ran.

mod common;

use std::collections::BTreeMap;

use common::world::{self, World, OWNER};
use rue_core::journal::{Entry, Event as J};
use rue_core::ledger::LedgerCode;
use rue_core::model::{
    Duration, FootprintEntry, Item, Kind, Output as OpOutput, PlanGate, StepI, Tri, Undo,
};
use rue_core::states::{RCode, State};
use rue_engine::executor::{Observation, Output, RPrim, Resolved, Scripted};
use rue_engine::journal::Sink;
use rue_engine::lifecycle::{applied_steps, ApplyOptions, EngineError};

fn opts() -> ApplyOptions {
    ApplyOptions {
        by: "ops".into(),
        ..ApplyOptions::default()
    }
}

fn kinds(events: &[J]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            let d = format!("{e:?}");
            d.split([' ', '{']).next().unwrap_or("").to_string()
        })
        .collect()
}

#[test]
fn a_temporary_plan_applies_its_steps_in_order_write_ahead_and_rests_applied() {
    let mut w = World::new("apply");
    let plan = world::temp_plan(
        "p",
        vec![world::step(world::op("a")), world::step(world::op("b"))],
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Applied, 0), "{}", out.line);
    assert_eq!(w.commands(), vec!["do a", "do b"]);
    let ev = w.sink.events();
    assert_eq!(
        kinds(&ev),
        vec![
            "Checked",
            "Requested",
            "Approved",
            "Applying",
            "StepDone",
            "Applying",
            "StepDone",
            "Applied"
        ]
    );
    assert!(matches!(&ev[3], J::Applying { step: 1, undo_line } if undo_line == "undo a"));
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(
        applied_steps(&rec).into_iter().collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(rec.deadline.map(|d| d.unix_s), Some(world::T0 + 3600));
    assert_eq!(w.engine.store().read_ledger().unwrap().holdings().len(), 1);
}

/// The write-ahead rule: the `Applying{n}` entry is acknowledged before the
/// fake runs step n. A sink that counts the fake's calls at delivery time
/// sees n-1 calls when entry n arrives.
struct CountingSink {
    ssh: rue_engine::executor::FakeHandle,
    seen: std::sync::Arc<std::sync::Mutex<Vec<(u32, usize)>>>,
}

impl Sink for CountingSink {
    fn name(&self) -> String {
        "counting".into()
    }
    fn deliver(&mut self, e: &Entry) -> Result<(), String> {
        if let J::Applying { step, .. } = &e.event {
            let n = self.ssh.calls().len();
            self.seen.lock().unwrap().push((*step, n));
        }
        Ok(())
    }
}

#[test]
fn the_write_ahead_entry_is_acknowledged_before_the_step_runs() {
    let w = World::new("write-ahead");
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let store = rue_engine::store::Store::create(&w.dir.join("store2")).unwrap();
    let sinks: Vec<Box<dyn Sink>> = vec![Box::new(CountingSink {
        ssh: w.ssh.clone(),
        seen: seen.clone(),
    })];
    let journal = rue_engine::journal::Journal::open(&store, sinks, None).unwrap();
    let execs: Vec<Box<dyn rue_engine::executor::Executor>> =
        vec![Box::new(w.ssh.clone()), Box::new(w.local.clone())];
    let mut engine = rue_engine::lifecycle::Engine::open(
        store,
        journal,
        w.clock.clone(),
        execs,
        vec![world::host(OWNER, &["ssh"])],
    )
    .unwrap();
    let plan = world::temp_plan(
        "p",
        vec![world::step(world::op("a")), world::step(world::op("b"))],
    );
    engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![(1, 0), (2, 1)]);
}

#[test]
fn a_failing_step_is_itself_reverted_then_the_prefix_and_the_ledger_is_released() {
    let mut w = World::new("refuse");
    w.ssh.script(vec![
        Scripted::Ok(Output::default()),
        Scripted::Fail("boom".into()),
    ]);
    let plan = world::temp_plan(
        "p",
        vec![world::step(world::op("a")), world::step(world::op("b"))],
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Closed, 1), "{}", out.line);
    assert_eq!(w.commands(), vec!["do a", "do b", "undo b", "undo a"]);
    let ev = w.sink.events();
    assert_eq!(
        kinds(&ev),
        vec![
            "Checked",
            "Requested",
            "Approved",
            "Applying",
            "StepDone",
            "Applying",
            "StepFailed",
            "Reverting",
            "Reverted",
            "Closed"
        ]
    );
    // Step 2 undid itself at failure; the revert covers the prefix.
    assert!(matches!(&ev[7], J::Reverting { steps } if steps == &vec![1]));
    assert!(w
        .engine
        .store()
        .read_ledger()
        .unwrap()
        .holdings()
        .is_empty());
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert!(rec.applied.is_empty());
    assert!(rec.closed_reason.unwrap().contains("reverted"));
}

#[test]
fn a_failing_undo_is_stuck_retried_each_pass_and_abandonable() {
    let mut w = World::new("stuck");
    w.ssh.script(vec![
        Scripted::Ok(Output::default()),
        Scripted::Fail("boom".into()),
        Scripted::Ok(Output::default()), // undo b
        Scripted::Fail("undo a broke".into()),
    ]);
    let plan = world::temp_plan(
        "p",
        vec![world::step(world::op("a")), world::step(world::op("b"))],
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Stuck, 4), "{}", out.line);
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.stuck, vec![1]);
    assert!(kinds(&w.sink.events()).ends_with(&["StepFailed".into(), "Stuck".into()]));
    assert_eq!(
        w.engine.store().read_ledger().unwrap().holdings().len(),
        1,
        "stuck still holds"
    );
    // The reap pass retries; it fails again; the notification is re-sent.
    w.ssh.script(vec![Scripted::Fail("still broke".into())]);
    let r = w.engine.reap().unwrap();
    assert_eq!(r.notify, vec![(out.id.clone(), State::Stuck)]);
    assert!(
        r.actions.iter().any(|a| a.contains("retried")),
        "{:?}",
        r.actions
    );
    assert_eq!(
        w.engine.status(&out.id).unwrap().unwrap().state,
        State::Stuck
    );
    // A pass where the undo works closes the instance.
    let r = w.engine.reap().unwrap();
    assert!(r.actions.iter().any(|a| a.contains("retried")));
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.state, State::Closed);
    assert_eq!(
        w.commands(),
        vec!["do a", "do b", "undo b", "undo a", "undo a", "undo a"]
    );
    assert!(w
        .engine
        .store()
        .read_ledger()
        .unwrap()
        .holdings()
        .is_empty());
}

#[test]
fn abandon_closes_a_stuck_instance_with_the_world_left_as_is() {
    let mut w = World::new("abandon");
    w.ssh.script(vec![
        Scripted::Fail("boom".into()),
        Scripted::Fail("undo broke".into()),
        Scripted::Fail("undo broke again".into()),
    ]);
    let plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Stuck, "{}", out.line);
    let out = w.engine.abandon(&out.id, "admin", "hardware gone").unwrap();
    assert_eq!((out.state, out.exit), (State::Closed, 1));
    let ev = w.sink.events();
    assert!(
        matches!(ev.iter().rev().nth(1), Some(J::Abandoned { steps_not_reverted, by, reason, .. }) if steps_not_reverted == &vec![1] && by == "admin" && reason == "hardware gone")
    );
    assert_eq!(
        w.commands(),
        vec!["do a", "undo a", "undo a"],
        "abandon runs nothing"
    );
    // Abandon anywhere else is not applicable.
    let plan = world::temp_plan("q", vec![world::step(world::op("c"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert!(matches!(
        w.engine.abandon(&out.id, "admin", "no"),
        Err(EngineError::WrongState {
            state: State::Applied,
            ..
        })
    ));
}

#[test]
fn an_executor_that_promises_output_and_returns_none_is_a_refusal() {
    let mut w = World::new("silent");
    let mut o = world::op("a");
    o.outputs = vec![OpOutput {
        name: "token".into(),
        secret: false,
    }];
    // The first run says nothing; the second (another plan) says the output.
    w.ssh.script(vec![Scripted::Ok(Output::default())]);
    let plan = world::temp_plan("p", vec![world::step(o.clone())]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Closed, 1), "{}", out.line);
    assert!(w.sink.events().iter().any(|e| matches!(e, J::StepFailed { error, .. } if error.contains("silent") && error.contains("token"))));
    assert_eq!(w.commands(), vec!["do a", "undo a"]);

    // The executor's own Silent is the same refusal.
    w.ssh.script(vec![Scripted::Silent]);
    let plan = world::temp_plan("q", vec![world::step(o.clone())]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed);

    // With the output present the step applies and the output is bound.
    let mut outputs = BTreeMap::new();
    outputs.insert("token".to_string(), "t-1".to_string());
    w.ssh.script(vec![Scripted::Ok(Output {
        stdout: String::new(),
        outputs,
    })]);
    let plan = world::temp_plan("r", vec![world::step(o)]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied);
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.outputs.get("a.token").map(String::as_str), Some("t-1"));
}

#[test]
fn wane_expires_a_temporary_plan_at_the_instant_and_reverts_it() {
    let mut w = World::new("wane");
    let plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    w.advance(3599);
    let r = w.engine.reap().unwrap();
    assert!(r.actions.is_empty(), "{:?}", r.actions);
    w.advance(1);
    let r = w.engine.reap().unwrap();
    assert_eq!(r.actions, vec![format!("{}: wane elapsed", out.id)]);
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.state, State::Closed);
    assert!(kinds(&w.sink.events()).ends_with(&[
        "Expired".into(),
        "Reverting".into(),
        "Reverted".into(),
        "Closed".into()
    ]));
    assert_eq!(w.commands(), vec!["do a", "undo a"]);
}

#[test]
fn renewal_is_within_the_window_never_after_expiry_and_anchored_at_renewal() {
    let mut w = World::new("renew");
    let plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let err = w.engine.renew(&out.id, Duration::new(3600)).unwrap_err();
    assert!(
        matches!(err, EngineError::NotAdmitted(RCode::R0102, _)),
        "{err}"
    );
    w.advance(3000);
    let r = w.engine.renew(&out.id, Duration::new(7200)).unwrap();
    assert_eq!(r.state, State::Applied);
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(
        rec.deadline.map(|d| d.unix_s),
        Some(world::T0 + 3000 + 7200)
    );
    assert!(w.sink.events().contains(&J::Renewed));
    w.advance(7200);
    w.engine.reap().unwrap();
    let err = w.engine.renew(&out.id, Duration::new(60)).unwrap_err();
    assert!(
        matches!(
            err,
            EngineError::WrongState {
                state: State::Closed,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn a_permanent_plan_confirms_commits_and_never_rests_and_a_temporary_one_refuses_commit() {
    let mut w = World::new("commit");
    let plan = world::perm_plan("p", vec![world::step(world::op("a")), Item::Confirm]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Committed, 0), "{}", out.line);
    assert_eq!(
        kinds(&w.sink.events()),
        vec![
            "Checked",
            "Requested",
            "Approved",
            "Applying",
            "StepDone",
            "Confirmed",
            "Committed"
        ]
    );
    assert!(w
        .engine
        .store()
        .read_ledger()
        .unwrap()
        .holdings()
        .is_empty());
    let err = w.engine.recant(&out.id, &[]).unwrap_err();
    assert!(matches!(
        err,
        EngineError::WrongState {
            state: State::Committed,
            ..
        }
    ));
    // A temporary plan: commit is not admitted (R0102), renew is.
    let plan = world::temp_plan("t", vec![world::step(world::op("b"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let err = w.engine.commit(&out.id, "ops", "why").unwrap_err();
    assert!(
        matches!(
            err,
            EngineError::WrongState { .. } | EngineError::NotAdmitted(RCode::R0102, _)
        ),
        "{err}"
    );
    let err = w.engine.confirm(&out.id).unwrap_err();
    assert!(
        matches!(err, EngineError::NotAdmitted(RCode::R0102, _)),
        "{err}"
    );
}

#[test]
fn a_plan_with_a_gate_is_pending_and_its_window_lapses_fail_closed() {
    let mut w = World::new("gate");
    let mut plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan.gate = Some(PlanGate {
        expr: rue_core::model::GateExpr::Single(rue_core::model::Factor::Auth {
            id: "oncall".into(),
            weight: 1,
        }),
        window: Some(Duration::new(1800)),
        allow_zero_human: false,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Pending, 6), "{}", out.line);
    assert!(w.commands().is_empty());
    assert_eq!(
        w.engine.store().read_ledger().unwrap().holdings().len(),
        1,
        "pending reserves"
    );
    w.advance(1800);
    let r = w.engine.reap().unwrap();
    assert_eq!(
        r.actions,
        vec![format!("{}: approval window lapsed", out.id)]
    );
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.state, State::Closed);
    assert!(w.sink.events().contains(&J::ApprovalExpired));
    assert!(w
        .engine
        .store()
        .read_ledger()
        .unwrap()
        .holdings()
        .is_empty());
    // Cancel while pending.
    let mut plan = world::temp_plan("q", vec![world::step(world::op("b"))]);
    plan.gate = Some(PlanGate {
        expr: rue_core::model::GateExpr::Single(rue_core::model::Factor::Auth {
            id: "oncall".into(),
            weight: 1,
        }),
        window: Some(Duration::new(1800)),
        allow_zero_human: false,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let out = w.engine.cancel(&out.id).unwrap();
    assert_eq!(out.state, State::Closed);
    assert!(w.sink.events().contains(&J::Cancelled));
}

#[test]
fn an_unknown_guard_waits_until_observed_yes_and_a_no_refuses() {
    let mut w = World::new("guard");
    let guarded = |id: &str| {
        let mut o = world::op(id);
        o.pre = vec![world::guard("ready", Tri::Unknown)];
        o
    };
    w.ssh.observe_as("ready", Observation::unknown("?"));
    let mut plan = world::temp_plan(
        "p",
        vec![world::step(guarded("a")), world::step(world::op("b"))],
    );
    plan.probes.push(world::probe("ready"));
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Waiting, 6), "{}", out.line);
    assert!(w.commands().is_empty());
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    let wait = rec.waiting.clone().unwrap();
    assert_eq!((wait.step, wait.guard.as_deref()), (1, Some("ready")));
    assert_eq!(
        wait.bound.map(|b| b.unix_s),
        Some(world::T0 + 600),
        "bounded by max_wait"
    );
    // Still unknown: nothing.
    w.engine.reap().unwrap();
    assert_eq!(
        w.engine.status(&out.id).unwrap().unwrap().state,
        State::Waiting
    );
    // Yes: the plan continues to the end.
    w.ssh.observe_as("ready", Observation::yes("ok"));
    let r = w.engine.reap().unwrap();
    assert!(
        r.actions.iter().any(|a| a.contains("now yes")),
        "{:?}",
        r.actions
    );
    assert_eq!(
        w.engine.status(&out.id).unwrap().unwrap().state,
        State::Applied
    );
    assert_eq!(w.commands(), vec!["do a", "do b"]);
    w.engine.recant(&out.id, &[]).unwrap();

    // A guard observed no refuses before anything runs.
    w.ssh.observe_as("ready", Observation::no("down"));
    let mut plan = world::temp_plan("q", vec![world::step(guarded("c"))]);
    plan.probes.push(world::probe("ready"));
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed);
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::Refused { reason } if reason.contains("guard ready is no"))));

    // A wait whose bound lapses reverts (on_lapse revert).
    w.ssh.observe_as("ready", Observation::unknown("?"));
    let mut plan = world::temp_plan("r", vec![world::step(guarded("d"))]);
    plan.probes.push(world::probe("ready"));
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Waiting);
    w.advance(600);
    let r = w.engine.reap().unwrap();
    assert!(
        r.actions.iter().any(|a| a.contains("lapsed")),
        "{:?}",
        r.actions
    );
    assert_eq!(
        w.engine.status(&out.id).unwrap().unwrap().state,
        State::Closed
    );
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::WaitLapsed { step: 1, .. })));

    // Forced by name in manual mode, an unknown guard passes.
    let mut plan = world::temp_plan("s", vec![world::step(guarded("e"))]);
    plan.probes.push(world::probe("ready"));
    let out = w
        .engine
        .apply(
            world::ir(plan),
            BTreeMap::new(),
            ApplyOptions {
                forced: vec!["ready".into()],
                ..opts()
            },
        )
        .unwrap();
    assert_eq!(out.state, State::Applied);
}

#[test]
fn a_refusal_after_a_holding_step_holds_and_resume_retries_the_failed_step() {
    let mut w = World::new("hold");
    w.ssh.script(vec![
        Scripted::Ok(Output::default()),
        Scripted::Fail("flaky".into()),
        Scripted::Ok(Output::default()), // step b's own undo
        Scripted::Ok(Output::default()), // step b again, on resume
    ]);
    let plan = world::temp_plan(
        "p",
        vec![
            world::step(world::hold(world::op("a"))),
            world::step(world::op("b")),
        ],
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Held, 3), "{}", out.line);
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.held_at, Some(2));
    assert_eq!(
        applied_steps(&rec).into_iter().collect::<Vec<_>>(),
        vec![1],
        "a stays applied"
    );
    assert!(w.sink.events().contains(&J::Held { step: 2 }));
    let out = w.engine.resume(&out.id, "ops").unwrap();
    assert_eq!(out.state, State::Applied);
    assert_eq!(w.commands(), vec!["do a", "do b", "undo b", "do b"]);
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::Resumed { step: 2, by } if by == "ops")));
    // Recant from Held reverts.
    w.ssh.script(vec![
        Scripted::Ok(Output::default()),
        Scripted::Fail("flaky".into()),
        Scripted::Ok(Output::default()),
    ]);
    let plan = world::temp_plan(
        "q",
        vec![
            world::step(world::hold(world::op("c"))),
            world::step(world::op("d")),
        ],
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Held);
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Closed);
    assert!(w.commands().ends_with(&[
        "do c".into(),
        "do d".into(),
        "undo d".into(),
        "undo c".into()
    ]));
}

#[test]
fn a_step_on_a_host_no_transport_reaches_is_deferred_and_handoff_done_continues() {
    let mut w = World::new("defer");
    let mut far = world::op("f");
    far = world::on(far, world::FAR);
    far.handoff_done = Some("far_done".into());
    let mut plan = world::temp_plan(
        "p",
        vec![
            world::step(world::op("a")),
            world::step(far),
            world::step(world::op("b")),
        ],
    );
    plan.probes.push(world::probe("far_done"));
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Deferred, 5), "{}", out.line);
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::Deferred { step: 2, handoff } if handoff.starts_with("far_done"))));
    // The wrong step is refused; the right one continues.
    assert!(matches!(
        w.engine.handoff_done(&out.id, 3, "ops"),
        Err(EngineError::WrongState { .. })
    ));
    let out = w.engine.handoff_done(&out.id, 2, "ops").unwrap();
    assert_eq!(out.state, State::Applied);
    assert_eq!(w.commands(), vec!["do a", "do b"]);
    // The handoff probe, observed yes by the reap pass, does the same.
    let mut far2 = world::on(world::op("g"), world::FAR);
    far2.handoff_done = Some("far_done".into());
    let mut plan = world::temp_plan("q", vec![world::step(far2)]);
    plan.probes.push(world::probe("far_done"));
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Deferred);
    w.ssh.observe_as("far_done", Observation::yes("done"));
    let r = w.engine.reap().unwrap();
    assert!(
        r.actions
            .iter()
            .any(|a| a.contains("handoff done by probe")),
        "{:?}",
        r.actions
    );
    assert_eq!(
        w.engine.status(&out.id).unwrap().unwrap().state,
        State::Applied
    );
}

#[test]
fn a_restore_undo_removes_the_owned_file_strips_the_region_and_writes_the_snapshot_back() {
    let mut w = World::new("restore");
    w.ssh.with(|f| {
        f.facts.insert("file:/etc/conf".into(), b"k=1\n".to_vec());
        f.facts.insert("file:/etc/shared".into(), b"top\n".to_vec());
    });
    let mut o = rue_core::model::Op::new(
        "cfg",
        vec![
            FootprintEntry::entry(Kind::Owned, "file:/etc/new"),
            FootprintEntry::anchored("file:/etc/shared", "blk"),
            FootprintEntry::entry(Kind::Modified, "file:/etc/conf"),
        ],
    );
    o.undo = Undo::Restore;
    o.do_ = vec![
        rue_core::body::Prim::Write(rue_core::body::Write {
            fact: rue_core::body::FactRef {
                shape: "file:/etc/new".into(),
                anchor: None,
            },
            content: rue_core::body::lit("hello"),
        }),
        rue_core::body::Prim::RegionSet(rue_core::body::RegionSet {
            fact: rue_core::body::FactRef {
                shape: "file:/etc/shared".into(),
                anchor: Some("blk".into()),
            },
            content: rue_core::body::lit("inside"),
        }),
        rue_core::body::Prim::Write(rue_core::body::Write {
            fact: rue_core::body::FactRef {
                shape: "file:/etc/conf".into(),
                anchor: None,
            },
            content: rue_core::body::lit("k=2\n"),
        }),
    ];
    let plan = world::temp_plan("p", vec![world::step(o)]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let after_do = w.ssh.with(|f| f.facts.clone());
    assert_eq!(after_do["file:/etc/new"], b"hello");
    assert_eq!(after_do["file:/etc/conf"], b"k=2\n");
    assert!(
        String::from_utf8_lossy(&after_do["file:/etc/shared"]).contains("# rue-region blk begin")
    );
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.snapshots.get("1/1").map(String::as_str), Some("top\n"));
    assert_eq!(rec.snapshots.get("1/2").map(String::as_str), Some("k=1\n"));
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Closed);
    let after_undo = w.ssh.with(|f| f.facts.clone());
    assert!(!after_undo.contains_key("file:/etc/new"));
    assert_eq!(after_undo["file:/etc/conf"], b"k=1\n");
    assert_eq!(after_undo["file:/etc/shared"], b"top\n");
    let undo = w.ssh.calls().last().unwrap().body.clone();
    assert_eq!(
        undo,
        vec![
            RPrim::Remove {
                shape: "file:/etc/new".into()
            },
            RPrim::RegionClear {
                shape: "file:/etc/shared".into(),
                anchor: Some("blk".into())
            },
            RPrim::Write {
                shape: "file:/etc/conf".into(),
                content: Resolved::plain("k=1\n")
            },
        ]
    );
}

#[test]
fn a_rehearsal_journals_every_step_calls_no_executor_and_reserves_nothing() {
    let mut w = World::new("rehearsal");
    let plan = world::temp_plan(
        "p",
        vec![world::step(world::op("a")), world::step(world::op("b"))],
    );
    let out = w
        .engine
        .apply(
            world::ir(plan),
            BTreeMap::new(),
            ApplyOptions {
                rehearsal: true,
                ..opts()
            },
        )
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Applied, 0));
    assert!(
        out.line.contains("rehearsal: no reservation"),
        "{}",
        out.line
    );
    assert!(w.commands().is_empty());
    assert!(w
        .engine
        .store()
        .read_ledger()
        .unwrap()
        .holdings()
        .is_empty());
    let ev = w.sink.events();
    assert!(ev.contains(&J::Approved { rehearsal: true }));
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, J::StepDone { .. }))
            .count(),
        2
    );
}

#[test]
fn a_rehearsal_neither_blocks_the_real_plan_nor_is_blocked_by_it() {
    let mut w = World::new("rehearsal-blocks");
    let plan = || world::temp_plan("p", vec![world::step(world::op("a"))]);
    let rehearse = || ApplyOptions {
        rehearsal: true,
        ..opts()
    };

    // Rehearse, then really apply. D-085 states the rule in its own
    // rationale -- "a rehearsal that blocks the real thing is not a
    // rehearsal" -- and the ledger is only one of the two ways to block
    // one. The instance store is the other: a rehearsal ends `Applied`,
    // which is not terminal, so sharing the id made the real plan
    // unapplicable for good the first time anybody rehearsed it.
    let dry = w
        .engine
        .apply(world::ir(plan()), BTreeMap::new(), rehearse())
        .unwrap();
    assert_eq!(dry.state, State::Applied);
    let real = w
        .engine
        .apply(world::ir(plan()), BTreeMap::new(), opts())
        .expect("a rehearsal must not stand in the way of the real plan");
    assert_eq!(real.state, State::Applied, "{}", real.line);
    assert_ne!(
        real.id, dry.id,
        "the two runs share an id, so the journal cannot tell them apart \
         and each overwrites the other's record"
    );

    // And the other direction, which is the worse one: rehearsing a plan
    // that is already running must not be refused, and must not overwrite
    // the live instance's record with one that holds nothing.
    let again = w
        .engine
        .apply(world::ir(plan()), BTreeMap::new(), rehearse())
        .expect("a rehearsal is never blocked (7.9)");
    assert_eq!(again.id, dry.id, "a rehearsal's id is deterministic too");
    let live = w.engine.status(&real.id).unwrap().unwrap();
    assert_eq!(live.state, State::Applied);
    assert!(
        !live.rehearsal,
        "the rehearsal overwrote the live instance's record"
    );
    assert!(
        !live.ledger_ids.is_empty(),
        "the live instance lost its reservations to a rehearsal"
    );

    // A real second apply is still refused, which is the guard doing its
    // actual job: only the rehearsal is exempt from it.
    let err = w
        .engine
        .apply(world::ir(plan()), BTreeMap::new(), opts())
        .unwrap_err();
    assert!(err.to_string().contains("R0101"), "{err}");
}

#[test]
fn a_second_instance_in_a_held_exclusivity_class_is_refused_r0101() {
    let mut w = World::new("exclusive");
    let mut plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan.exclusivity = Some("cls".into());
    w.engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let mut plan2 = world::temp_plan("q", vec![world::step(world::op("b"))]);
    plan2.exclusivity = Some("cls".into());
    let err = w
        .engine
        .apply(world::ir(plan2), BTreeMap::new(), opts())
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Ledger(LedgerCode::R0101, _)),
        "{err}"
    );
    // The same plan again while it is active: also refused.
    let mut plan3 = world::temp_plan("p", vec![world::step(world::op("a"))]);
    plan3.exclusivity = Some("cls".into());
    let err = w
        .engine
        .apply(world::ir(plan3), BTreeMap::new(), opts())
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Ledger(LedgerCode::R0101, _)),
        "{err}"
    );
    // An overlapping umbra, no class: R0203.
    let plan4 = world::temp_plan("r", vec![world::step(world::op("a"))]);
    let err = w
        .engine
        .apply(world::ir(plan4), BTreeMap::new(), opts())
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Ledger(LedgerCode::R0203, _)),
        "{err}"
    );
}

#[test]
fn boot_demotes_an_instance_left_applying_and_reverts_it() {
    let mut w = World::new("boot");
    let plan = world::temp_plan(
        "p",
        vec![world::step(world::op("a")), world::step(world::op("b"))],
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    // A crash after step 1's write-ahead: the record says Applying with
    // step 1 applied and step 2 owed.
    let mut rec = w.engine.status(&out.id).unwrap().unwrap();
    rec.state = State::Applying;
    rec.applied.truncate(1);
    w.engine.store().write_instance(&rec.id, &rec).unwrap();
    let mut w = w.restart();
    let report = w.engine.boot().unwrap();
    assert_eq!(report.demoted, vec![out.id.clone()]);
    assert!(!w.engine.settling());
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.state, State::Closed);
    assert_eq!(w.commands(), vec!["do a", "do b", "undo a"]);
    assert!(kinds(&w.sink.events()).ends_with(&[
        "Reverting".into(),
        "Reverted".into(),
        "Closed".into()
    ]));
}

#[test]
fn during_settle_no_wane_fires_and_held_resources_are_reestablished_first() {
    let mut w = World::new("settle");
    let mut o = world::op("tunnel");
    o.footprint = vec![FootprintEntry::entry(Kind::Held, "proc:tunnel")];
    o.suspend = Some(world::run("suspend"));
    o.reestablish = Some(world::run("resume"));
    let plan = world::temp_plan("p", vec![world::step(o)]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied);
    // The engine was down past the wane.
    w.advance(4000);
    let mut w = w.restart();
    // A reap pass before settle ends must not fire the wane: boot itself
    // reaps nothing, and the report says it settled.
    let report = w.engine.boot().unwrap();
    assert_eq!(report.reestablished, vec![(out.id.clone(), 1)]);
    let ev = w.sink.events();
    let i_re = ev.iter().position(|e| *e == J::Reestablished).unwrap();
    assert!(!ev.contains(&J::Expired), "wane fired during settle");
    // After settle the wane fires.
    let r = w.engine.reap().unwrap();
    assert!(!r.settling);
    assert!(
        r.actions.iter().any(|a| a.contains("wane elapsed")),
        "{:?}",
        r.actions
    );
    let ev = w.sink.events();
    let i_ex = ev.iter().position(|e| *e == J::Expired).unwrap();
    assert!(i_re < i_ex);
    assert_eq!(w.commands(), vec!["do tunnel", "resume", "undo tunnel"]);
    // A resource that cannot be reestablished suspends the instance.
    let mut w2 = World::new("settle-lost");
    let mut o = world::op("tunnel");
    o.footprint = vec![FootprintEntry::entry(Kind::Held, "proc:tunnel")];
    o.suspend = Some(world::run("suspend"));
    o.reestablish = Some(world::run("resume"));
    let plan = world::temp_plan("p", vec![world::step(o)]);
    let out = w2
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    w2.ssh.script(vec![Scripted::Fail("gone".into())]);
    let mut w2 = w2.restart();
    let report = w2.engine.boot().unwrap();
    assert_eq!(report.lost, vec![(out.id.clone(), 1)]);
    assert_eq!(
        w2.engine.status(&out.id).unwrap().unwrap().state,
        State::Suspended
    );
    assert!(w2.sink.events().contains(&J::Suspended));
    // Next boot, the resource is back: Reestablished, Applied again.
    let mut w2 = w2.restart();
    let report = w2.engine.boot().unwrap();
    assert_eq!(report.reestablished, vec![(out.id.clone(), 1)]);
    assert_eq!(
        w2.engine.status(&out.id).unwrap().unwrap().state,
        State::Applied
    );
}

#[test]
fn the_settle_flag_survives_a_crash_during_boot() {
    let mut w = World::new("settle-crash");
    let plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let mut m = BTreeMap::new();
    m.insert("settling".to_string(), "true".to_string());
    w.engine.store().write_meta("settle", &m).unwrap();
    w.advance(4000);
    let mut w = w.restart();
    assert!(w.engine.settling());
    let r = w.engine.reap().unwrap();
    assert!(r.settling && r.actions.is_empty(), "{r:?}");
    assert_eq!(
        w.engine.status(&out.id).unwrap().unwrap().state,
        State::Applied
    );
    w.engine.boot().unwrap();
    let r = w.engine.reap().unwrap();
    assert!(r.actions.iter().any(|a| a.contains("wane elapsed")));
}

#[test]
fn a_migrated_store_is_journaled_at_the_first_boot() {
    let w = World::new("migrated");
    let mut m = BTreeMap::new();
    m.insert("from".to_string(), "0".to_string());
    m.insert("to".to_string(), "1".to_string());
    m.insert("by".to_string(), "admin".to_string());
    w.engine.store().write_meta("migrated", &m).unwrap();
    let mut w = w.restart();
    let report = w.engine.boot().unwrap();
    assert_eq!(report.migrated, Some((0, 1)));
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::Migrated { from: 0, to: 1, by } if by == "admin")));
    let report = w.engine.boot().unwrap();
    assert_eq!(report.migrated, None, "journaled once");
}

#[test]
fn a_plan_the_check_refuses_never_reaches_the_store() {
    let mut w = World::new("refused-check");
    // No wane and no commit: E0501.
    let plan = rue_core::model::Plan::new("p", OWNER, vec![world::step(world::op("a"))]);
    let err = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap_err();
    assert!(matches!(err, EngineError::Refused(_)), "{err}");
    assert!(w.engine.instances().unwrap().is_empty());
    assert!(w.sink.events().is_empty());
}

#[test]
fn a_when_chooses_its_arm_once_and_a_repeat_runs_its_body_per_item() {
    let mut w = World::new("when-repeat");
    w.ssh.observe_as("cold", Observation::no("warm"));
    let mut b_op = world::op("b");
    b_op.do_ = vec![rue_core::body::Prim::Run(rue_core::body::Run {
        cmd: vec![
            rue_core::body::Part::Lit("do b on ".into()),
            rue_core::body::Part::Ref(rue_core::body::controller("g")),
        ],
        env: vec![],
        stdin: None,
    })];
    let mut plan = world::temp_plan(
        "p",
        vec![
            Item::When {
                guard: world::guard("cold", Tri::Unknown),
                window: None,
                on_lapse: rue_core::model::OnLapse::Revert,
                then_: vec![world::step(world::op("heat"))],
                else_: vec![world::step(world::op("a"))],
            },
            Item::Repeat {
                form: rue_core::model::RepeatForm::Over {
                    list: "guests".into(),
                    max: 3,
                    set_valued: true,
                },
                var: "g".into(),
                body: vec![world::step(b_op)],
            },
        ],
    );
    plan.probes.push(world::probe("cold"));
    let mut params = BTreeMap::new();
    params.insert("guests".to_string(), "g1, g2".to_string());
    let out = w.engine.apply(world::ir(plan), params, opts()).unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    assert_eq!(w.commands(), vec!["do a", "do b on g1", "do b on g2"]);
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.choices.get("w1"), Some(&false));
    assert_eq!(rec.applied.len(), 3);
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Closed);
    assert_eq!(w.commands()[3..], ["undo b", "undo b", "undo a"]);
}

#[test]
fn a_repeat_s_steps_are_undone_each_with_its_own_item() {
    // Each iteration of a repeat is its own applied step, with its own
    // value of the variable: its undo must name the guest IT started. The
    // undo ran with no controller values at all, so an undo reading the
    // variable -- T2's `jail -r rue-t2-#{g}` -- could not resolve, and a
    // recant of a promote that had started any guest left it Stuck. Nothing
    // had ever reverted a repeat whose undo said which item it was undoing.
    let mut w = World::new("repeat-undo");
    let item = |verb: &str| {
        vec![rue_core::body::Prim::Run(rue_core::body::Run {
            cmd: vec![
                rue_core::body::Part::Lit(format!("{verb} b on ")),
                rue_core::body::Part::Ref(rue_core::body::controller("g")),
            ],
            env: vec![],
            stdin: None,
        })]
    };
    let mut b_op = world::op("b");
    b_op.do_ = item("do");
    b_op.undo = Undo::Computed {
        body: item("undo"),
        undo_pre: vec!["file:/b".into()],
    };
    let plan = world::temp_plan(
        "p",
        vec![
            world::step(world::op("a")),
            Item::Repeat {
                form: rue_core::model::RepeatForm::Over {
                    list: "guests".into(),
                    max: 3,
                    set_valued: true,
                },
                var: "g".into(),
                body: vec![world::step(b_op)],
            },
        ],
    );
    let mut params = BTreeMap::new();
    params.insert("guests".to_string(), "g1, g2".to_string());
    let out = w.engine.apply(world::ir(plan), params, opts()).unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    assert_eq!(w.commands(), vec!["do a", "do b on g1", "do b on g2"]);
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert_eq!(
        w.commands()[3..],
        ["undo b on g2", "undo b on g1", "undo a"],
        "each iteration undone with its own item, the last first"
    );
}

#[test]
fn each_iteration_of_nested_repeats_touches_and_restores_the_fact_it_names() {
    // A fact whose shape names a repeat variable is a different fact per
    // iteration: `file:/conf/{o}-{i}` is four files over two lists of two.
    // Three things were wrong, each hiding the next. The engine used the
    // shape as written, so every iteration wrote and snapshotted one file
    // literally named `{o}-{i}`; it keyed markers and snapshots by step
    // number, so each iteration overwrote the last one's; and it told
    // applications apart by step and iteration alone, which repeat under
    // nesting, so the second outer pass found its inner steps "already
    // applied" and skipped them.
    let mut w = World::new("nested-repeat");
    let shape = "file:/conf/{o}-{i}";
    let mut o = rue_core::model::Op::new(
        "conf",
        vec![rue_core::model::FootprintEntry::entry(
            Kind::Modified,
            shape,
        )],
    );
    o.do_ = vec![rue_core::body::Prim::Write(rue_core::body::Write {
        fact: rue_core::body::FactRef {
            shape: shape.into(),
            anchor: None,
        },
        content: rue_core::body::lit("new"),
    })];
    o.undo = Undo::Restore;
    let over = |list: &str, var: &str, body: Vec<Item>| Item::Repeat {
        form: rue_core::model::RepeatForm::Over {
            list: list.into(),
            max: 3,
            set_valued: true,
        },
        var: var.into(),
        body,
    };
    let plan = world::temp_plan(
        "p",
        vec![over(
            "outer",
            "o",
            vec![over("inner", "i", vec![world::step(o)])],
        )],
    );
    let files = ["a-x", "a-y", "b-x", "b-y"];
    w.ssh.with(|f| {
        for n in files {
            f.facts
                .insert(format!("file:/conf/{n}"), format!("old {n}").into_bytes());
        }
    });
    let mut params = BTreeMap::new();
    params.insert("outer".to_string(), "a, b".to_string());
    params.insert("inner".to_string(), "x, y".to_string());
    let out = w.engine.apply(world::ir(plan), params, opts()).unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let now = |w: &World| -> Vec<(String, String)> {
        w.ssh.with(|f| {
            f.facts
                .iter()
                .map(|(k, v)| (k.clone(), String::from_utf8_lossy(v).into_owned()))
                .collect()
        })
    };
    assert_eq!(
        now(&w),
        files
            .iter()
            .map(|n| (format!("file:/conf/{n}"), "new".to_string()))
            .collect::<Vec<_>>(),
        "every iteration wrote its own file, and nothing else"
    );
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert_eq!(rec.applied.len(), 4, "four applications, none skipped");
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert_eq!(
        now(&w),
        files
            .iter()
            .map(|n| (format!("file:/conf/{n}"), format!("old {n}")))
            .collect::<Vec<_>>(),
        "each restored from its own snapshot"
    );
}

#[test]
fn a_knell_waits_for_its_acknowledgement_unless_acked_up_front() {
    let mut w = World::new("knell");
    let mut k = world::op("fence");
    k.refusal = rue_core::model::Refusal::Knell {
        guard: None,
        cost: rue_core::model::Cost::Probe("blast".into()),
        ack: rue_core::model::Ack::Gate(rue_core::model::GateExpr::Single(
            rue_core::model::Factor::Humans { weight: 1 },
        )),
    };
    k.undo = Undo::NoUndo;
    let mut plan = world::temp_plan(
        "p",
        vec![
            world::step(world::op("a")),
            Item::Knell(StepI::new(k.clone())),
        ],
    );
    plan.probes.push(world::probe("blast"));
    // The cost is measured where it is asked about, so the acknowledger
    // sees what the probe reports and not only its name.
    w.ssh
        .observe_as("blast", Observation::yes("two racks go dark"));
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Waiting, 6), "{}", out.line);
    assert!(w.sink.events().iter().any(
        |e| matches!(e, J::AckRequested { step: 2, cost } if cost == "blast: two racks go dark")
    ));
    assert_eq!(w.commands(), vec!["do a"]);
    let mut k2 = k.clone();
    k2.id = "fence2".into();
    k2.footprint = vec![FootprintEntry::entry(Kind::Owned, "file:/fence2")];
    let mut plan = world::temp_plan("q", vec![Item::Knell(StepI::new(k2))]);
    plan.probes.push(world::probe("blast"));
    let out = w
        .engine
        .apply(
            world::ir(plan),
            BTreeMap::new(),
            ApplyOptions {
                acks: vec![1],
                ..opts()
            },
        )
        .unwrap();
    assert_eq!(out.state, State::Applied);
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::KnellAcknowledged { step: 1, by, .. } if by == "ops")));
}

#[test]
fn a_step_whose_do_the_engine_died_inside_is_undone_on_the_way_back() {
    // The write-ahead entry exists for exactly this: the engine says what
    // it is about to do and how it would undo it, then does it. A death
    // between those two leaves a step that may have half happened and was
    // never marked applied, and boot recovery must undo it anyway (5.9,
    // 7.8). Undoing a step that never took is harmless; leaving one that
    // did is not.
    let mut w = World::new("boot-interrupted");
    let plan = world::temp_plan(
        "p",
        vec![world::step(world::op("a")), world::step(world::op("b"))],
    );
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    // Rewrite the record as a death mid-`do` of step 2 leaves it: step 1
    // applied, step 2 attempted and unmarked, the instance Applying.
    let mut rec = w.engine.status(&id).unwrap().unwrap();
    rec.state = State::Applying;
    rec.applied.retain(|a| a.step == 1);
    rec.attempting = Some(rue_engine::lifecycle::AppliedStep::new(
        2,
        0,
        &BTreeMap::new(),
    ));
    w.engine.store().write_instance(&id, &rec).unwrap();

    let mut w = w.restart();
    let boot = w.engine.boot().unwrap();
    assert_eq!(
        boot.interrupted,
        vec![(id.clone(), 2)],
        "the interrupted step is named"
    );
    assert!(boot.demoted.contains(&id), "{boot:?}");
    // Both steps are undone, the interrupted one first.
    let undos: Vec<String> = w
        .commands()
        .into_iter()
        .filter(|c| c.starts_with("undo "))
        .collect();
    assert_eq!(undos, vec!["undo b".to_string(), "undo a".to_string()]);
    let rec = w.engine.status(&id).unwrap().unwrap();
    assert_eq!(rec.state, State::Closed, "{rec:?}");
    assert!(rec.applied.is_empty() && rec.attempting.is_none());
}
