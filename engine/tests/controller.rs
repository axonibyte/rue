//! Where a `:controller` step goes: to a hook the site binds with
//! `transport: :controller` when there is one, and to `local()` otherwise.
//!
//! The controller was hard-wired to `local()`, which refuses a hook's
//! action. T2's fence, resurrection gate and succession record are all
//! `:controller` steps whose bodies are hook actions, so T2 checked clean
//! for four phases and could not have run one of them. E0608 now refuses
//! the plan when nothing can perform such an action; this is the half
//! that gives the controller something that can.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::world::{self, OWNER, T0};
use common::TempDir;
use rue_core::body::{Hook, Prim};
use rue_core::diagnostics::Code;
use rue_core::model::{Instant, Locus};
use rue_core::states::State;
use rue_engine::clock::FakeClock;
use rue_engine::executor::{Executor, FakeExecutor, FakeHandle, LocusKind};
use rue_engine::journal::{Journal, MemorySink, Sink};
use rue_engine::lifecycle::{ApplyOptions, Engine, EngineError};
use rue_engine::store::Store;

/// An engine with `local()` and, if asked, a hook bound to the controller.
fn engine(name: &str, controller_hook: bool) -> (TempDir, Engine, FakeHandle, Option<FakeHandle>) {
    let dir = TempDir::new(name);
    let store = Store::create(&dir.join("store")).unwrap();
    let sinks: Vec<Box<dyn Sink>> = vec![Box::new(MemorySink::new("mem"))];
    let journal = Journal::open(&store, sinks, None).unwrap();
    let clock = Arc::new(FakeClock::at(Instant::new(T0)));
    let local = FakeExecutor::new(LocusKind::Local).shared();
    let mut execs: Vec<Box<dyn Executor>> = vec![Box::new(local.clone())];
    let ctl =
        controller_hook.then(|| FakeExecutor::new(LocusKind::Hook("controller".into())).shared());
    if let Some(c) = &ctl {
        execs.push(Box::new(c.clone()));
    }
    let engine = Engine::open(
        store,
        journal,
        clock,
        execs,
        vec![world::host(OWNER, &["ssh"])],
    )
    .unwrap();
    (dir, engine, local, ctl)
}

/// A plan of one `:controller` step whose do is a hook's action -- T2's
/// fence, in the small.
fn fence_plan(controller_hook: bool) -> rue_core::ir::PlanIr {
    let mut o = world::op("fence");
    o.locus = Locus::Controller;
    o.footprint = vec![rue_core::model::FootprintEntry::entry(
        rue_core::model::Kind::Modified,
        "fence:state:node-a",
    )];
    o.do_ = vec![Prim::Hook(Hook {
        name: "fence".into(),
        args: Vec::new(),
    })];
    // A reversible step: its undo is the hook's too (an irreversible one
    // would have to be a knell, E0201), so both directions go to whoever
    // serves the controller.
    o.undo = rue_core::model::Undo::Computed {
        body: vec![Prim::Hook(Hook {
            name: "unfence".into(),
            args: Vec::new(),
        })],
        undo_pre: vec!["fence:state:node-a".into()],
    };
    o.undo_idempotent = true;
    let mut ir = world::ir(world::temp_plan("p", vec![world::step(o)]));
    if controller_hook {
        ir.site.transports.push("controller".into());
    }
    ir
}

fn opts() -> ApplyOptions {
    ApplyOptions {
        by: "ops".into(),
        ..ApplyOptions::default()
    }
}

fn calls(h: &FakeHandle) -> Vec<String> {
    h.with(|f| f.calls.iter().map(|c| c.host.clone()).collect())
}

#[test]
fn a_controller_step_goes_to_the_controller_hook_when_the_site_binds_one() {
    let (_d, mut engine, local, ctl) = engine("controller-hook", true);
    let ctl = ctl.unwrap();
    let out = engine
        .apply(fence_plan(true), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    assert_eq!(
        calls(&ctl),
        vec!["controller".to_string()],
        "the fence went nowhere near the hook bound to the controller"
    );
    assert!(
        calls(&local).is_empty(),
        "local() was asked to perform a hook's action: {:?}",
        calls(&local)
    );
}

#[test]
fn with_no_controller_hook_the_controller_is_still_local() {
    // The fallback is unchanged: a site that binds nothing to the controller
    // keeps local() there, and the checker is what refuses a hook action it
    // could not perform (E0608).
    let (_d, mut engine, local, _) = engine("controller-local", false);
    let err = engine
        .apply(fence_plan(false), BTreeMap::new(), opts())
        .unwrap_err();
    let codes: Vec<Code> = match &err {
        EngineError::Refused(v) => v.diagnostics.iter().map(|d| d.code).collect(),
        other => panic!("refused by something other than the check: {other:?}"),
    };
    // Two actions nothing here can perform -- the fence in the do body and
    // the unfence in the undo -- and each is named, so an operator sees both
    // rather than fixing one and meeting the other at revert.
    assert_eq!(
        codes,
        vec![Code::E0608, Code::E0608],
        "a hook action with only local() at the controller must be refused by the check"
    );
    assert!(
        calls(&local).is_empty(),
        "nothing may run once the check refuses"
    );
}
