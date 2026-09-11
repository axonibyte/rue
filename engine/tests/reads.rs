//! Facts read honestly, and a failed step undone only when its `do` took
//! (ROADMAP Phase 5, second unit; 5.2, 5.9).
//!
//! A read that fails -- here the fake's `failing_reads`, a connection that
//! drops -- is R0205 wherever the engine reads a fact, and never the fact's
//! absence: before `do` it refuses the step, after `do` it fails the step,
//! and at undo it fails the undo, which says so. Read as absent, a failed
//! snapshot kept nothing for a restore to put back, a failed marker hid
//! drift, and a failed read beside a real marker was drift nobody made.
//!
//! A failed step is undone at once because it may be half-applied; one
//! whose `do` left every fact it observes as it found them did not happen,
//! and is not undone. Its undo is the op's text written in advance, and one
//! that acts by name removes the object of that name whoever made it.

mod common;

use std::collections::BTreeMap;

use common::world::{self, World};
use rue_core::body::{lit, FactRef, Part, Prim, Ref, Run, Write};
use rue_core::journal::Event as J;
use rue_core::model::{Drift, FootprintEntry, Kind, Locus, Op, ProbeDecl, Undo, UndoLocus};
use rue_core::states::State;
use rue_engine::executor::{Observation, RPrim, Scripted};
use rue_engine::lifecycle::ApplyOptions;

fn opts() -> ApplyOptions {
    ApplyOptions {
        by: "ops".into(),
        ..ApplyOptions::default()
    }
}

/// An op that modifies `file:/conf` and restores it by footprint.
fn conf(drift: Option<Drift>) -> Op {
    let mut o = Op::new(
        "cfg",
        vec![FootprintEntry::entry(Kind::Modified, "file:/conf")],
    );
    o.undo = Undo::Restore;
    o.undo_locus = UndoLocus::Controller;
    o.drift = drift;
    o.do_ = vec![Prim::Write(Write {
        fact: FactRef {
            shape: "file:/conf".into(),
            anchor: None,
        },
        content: lit("k=2\n"),
    })];
    o
}

fn fact(w: &World, shape: &str) -> Option<String> {
    w.ssh
        .with(|f| f.facts.get(shape).cloned())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn set(w: &World, shape: &str, text: &str) {
    w.ssh
        .with(|f| f.facts.insert(shape.into(), text.as_bytes().to_vec()));
}

/// Reads of `shape` that succeed before every later one fails.
fn fail_reads(w: &World, shape: &str, after: u32) {
    w.ssh.with(|f| f.failing_reads.insert(shape.into(), after));
}

fn runs(w: &World) -> usize {
    w.ssh.calls().len()
}

fn journaled(w: &World, pred: impl Fn(&J) -> bool) -> bool {
    w.sink.events().iter().any(pred)
}

#[test]
fn a_fact_that_cannot_be_read_before_do_refuses_the_step_and_nothing_runs() {
    let mut w = World::new("reads-before");
    set(&w, "file:/conf", "k=1\n");
    fail_reads(&w, "file:/conf", 0);
    let plan = world::temp_plan("p", vec![world::step(conf(None))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Closed, 1), "{}", out.line);
    assert_eq!(runs(&w), 0, "do ran with nothing to put back");
    assert!(
        journaled(
            &w,
            |e| matches!(e, J::Refused { reason } if reason.contains("R0205") && reason.contains("file:/conf"))
        ),
        "{:?}",
        w.sink.events()
    );
    assert_eq!(fact(&w, "file:/conf").as_deref(), Some("k=1\n"));
}

#[test]
fn a_fact_that_cannot_be_read_after_do_fails_the_step_rather_than_marking_it() {
    let mut w = World::new("reads-after");
    set(&w, "file:/conf", "k=1\n");
    // The snapshot's read succeeds; the marker's, after do, does not.
    fail_reads(&w, "file:/conf", 1);
    let plan = world::temp_plan("p", vec![world::step(conf(None))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_ne!(out.state, State::Applied, "{}", out.line);
    assert!(
        journaled(
            &w,
            |e| matches!(e, J::StepFailed { step: 1, error } if error.contains("R0205"))
        ),
        "{:?}",
        w.sink.events()
    );
}

#[test]
fn a_fact_that_cannot_be_read_at_undo_is_stuck_not_judged_absent() {
    let mut w = World::new("reads-undo");
    set(&w, "file:/conf", "k=1\n");
    let plan = world::temp_plan("p", vec![world::step(conf(Some(Drift::Defer)))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    fail_reads(&w, "file:/conf", 0);
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Stuck, "{}", out.line);
    assert!(
        !journaled(&w, |e| matches!(
            e,
            J::DriftHeld { .. } | J::DriftClobbered { .. }
        )),
        "an unreadable fact was judged as drift: {:?}",
        w.sink.events()
    );
    assert!(
        journaled(
            &w,
            |e| matches!(e, J::StepFailed { error, .. } if error.contains("R0205"))
        ),
        "{:?}",
        w.sink.events()
    );
    assert_eq!(
        fact(&w, "file:/conf").as_deref(),
        Some("k=2\n"),
        "nothing was undone"
    );
}

#[test]
fn a_watched_fact_that_cannot_be_read_before_do_refuses_the_next_step() {
    let mut w = World::new("reads-watched");
    // Step 2's do must leave step 1's fact alone (R0201), which the engine
    // checks by reading it before and after. Unreadable before, step 2 is
    // refused and never runs.
    let first = world::op("a");
    let mut second = world::op("b");
    second.id = "b".into();
    set(&w, "file:/a", "x");
    // Step 1's snapshot and marker read it; step 2's witness does not get to.
    fail_reads(&w, "file:/a", 2);
    let plan = world::temp_plan("p", vec![world::step(first), world::step(second)]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert!(
        !w.commands().contains(&"do b".to_string()),
        "{:?}",
        w.commands()
    );
    assert!(
        journaled(
            &w,
            |e| matches!(e, J::Refused { reason } if reason.starts_with("step 2") && reason.contains("R0205"))
        ),
        "{:?}",
        w.sink.events()
    );
    assert_ne!(out.state, State::Applied);
}

#[test]
fn a_failed_step_whose_do_never_took_is_not_undone() {
    let mut w = World::new("never-took");
    // An object of the step's name is already there, made by someone else:
    // T2's obstacle jail, in the small. The `do` fails on it and changes
    // nothing, so the undo -- which acts by name -- must not run.
    set(&w, "file:/a", "theirs");
    w.ssh.script(vec![Scripted::Fail("already exists".into())]);
    let plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Closed, 1), "{}", out.line);
    assert_eq!(
        w.commands(),
        vec!["do a"],
        "the undo ran on what it never made"
    );
    assert!(
        journaled(&w, |e| matches!(e, J::UndoSkipped { step: 1, .. })),
        "{:?}",
        w.sink.events()
    );
    assert_eq!(fact(&w, "file:/a").as_deref(), Some("theirs"));
}

#[test]
fn a_failed_step_whose_do_took_something_is_undone() {
    let mut w = World::new("half-took");
    w.ssh.script(vec![Scripted::FailHaving(
        "partway".into(),
        vec![("file:/a".into(), b"half".to_vec())],
    )]);
    let plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert_eq!(w.commands(), vec!["do a", "undo a"]);
    assert!(!journaled(&w, |e| matches!(e, J::UndoSkipped { .. })));
}

#[test]
fn a_failed_step_whose_facts_cannot_be_read_again_is_not_shown_untouched() {
    let mut w = World::new("reads-unknown");
    // The snapshot reads; after the failure the fact cannot be read, so
    // nobody can say the do did not take: the undo is attempted, and its
    // own read fails and says so.
    fail_reads(&w, "file:/a", 1);
    w.ssh.script(vec![Scripted::Fail("boom".into())]);
    let plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Stuck, "{}", out.line);
    assert!(!journaled(&w, |e| matches!(e, J::UndoSkipped { .. })));
    assert!(journaled(
        &w,
        |e| matches!(e, J::StepFailed { error, .. } if error.contains("undo") && error.contains("R0205"))
    ));
}

/// T2's `start_guest` in the small: a jail's running state, a fact that is
/// no file, on a host reached over ssh, started by name and stopped by name.
fn start_guest() -> Op {
    let mut o = Op::new(
        "start_guest",
        vec![FootprintEntry::entry(Kind::Modified, "guest:state:g1")],
    );
    o.do_ = vec![Prim::Run(Run {
        cmd: vec![Part::Lit("jail -c name=g1 persist".into())],
        env: vec![],
        stdin: None,
    })];
    o.undo = Undo::Computed {
        body: vec![Prim::Run(Run {
            cmd: vec![Part::Lit("jail -r g1".into())],
            env: vec![],
            stdin: None,
        })],
        undo_pre: vec!["guest:state:g1".into()],
    };
    o.undo_locus = UndoLocus::Controller;
    o.drift = Some(Drift::Defer);
    o
}

/// `defprobe :guest_state do run "jls -j #{g} jid"; reads guest.state(g) end`
fn guest_state() -> ProbeDecl {
    ProbeDecl {
        name: "guest_state".into(),
        locus: Locus::Target,
        body: vec![Prim::Run(Run {
            cmd: vec![
                Part::Lit("jls -j ".into()),
                Part::Ref(Ref::Controller("g".into())),
                Part::Lit(" jid".into()),
            ],
            env: vec![],
            stdin: None,
        })],
        produces: vec![],
        static_: false,
        equivalence: "bytes".into(),
        reads: Some("guest:state:{g}".into()),
    }
}

#[test]
fn a_fact_that_is_no_file_is_read_over_ssh_by_the_probe_that_reads_it() {
    let mut w = World::new("reads-probe");
    // The fake answers any shape from its table when asked directly, which
    // ssh() cannot; the jail's state is kept off that table, so only the
    // probe can see it.
    w.ssh.observe_as("guest_state", Observation::yes("5"));
    let mut plan = world::temp_plan("p", vec![world::step(start_guest())]);
    plan.probes.push(guest_state());
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let ran = w.ssh.with(|f| f.probe_bodies.clone());
    assert!(
        ran.iter().any(|(name, body)| name == "guest_state"
            && matches!(body.as_slice(), [RPrim::Run { cmd, .. }] if cmd.text == "jls -j g1 jid")),
        "the probe ran with the fact's instance bound: {ran:?}"
    );
    // Someone restarts the jail by hand: another jid. Under :defer that is
    // drift to hold on, and `jail -r` must not run.
    w.ssh.observe_as("guest_state", Observation::yes("9"));
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert!(
        journaled(
            &w,
            |e| matches!(e, J::DriftHeld { facts, .. } if facts.contains(&"guest:state:g1".to_string()))
        ),
        "{}: {:?}",
        out.line,
        w.sink.events()
    );
    assert!(
        !w.commands().contains(&"jail -r g1".to_string()),
        "{:?}",
        w.commands()
    );
}

#[test]
fn a_reading_probe_that_cannot_tell_is_a_read_that_failed() {
    let mut w = World::new("reads-probe-unknown");
    w.ssh.observe_as(
        "guest_state",
        Observation::unknown("jls: permission denied"),
    );
    let mut plan = world::temp_plan("p", vec![world::step(start_guest())]);
    plan.probes.push(guest_state());
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(runs(&w), 0, "{}", out.line);
    assert!(
        journaled(
            &w,
            |e| matches!(e, J::Refused { reason } if reason.contains("R0205") && reason.contains("guest_state"))
        ),
        "{:?}",
        w.sink.events()
    );
}
