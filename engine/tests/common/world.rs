//! A small world for lifecycle tests: one target host reached by a fake
//! `ssh` executor, a fake `local` executor for the controller, a memory
//! sink, a fake clock, and builders for the ops and plans the scenarios use.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use rue_core::body::{Body, Part, Prim, Run};
use rue_core::ir::{PlanIr, IR_VERSION};
use rue_core::model::{
    Duration, FootprintEntry, Guard, HostRecord, Item, Kind, Locus, Op, Plan, ProbeDecl, Refusal,
    Site, StepI, Tri, Undo,
};
use rue_engine::clock::FakeClock;
use rue_engine::executor::{Executor, FakeExecutor, FakeHandle, LocusKind};
use rue_engine::host::Host;
use rue_engine::journal::{Journal, MemorySink, Sink};
use rue_engine::lifecycle::Engine;
use rue_engine::scheduler::FakeSchedulerHandle;
use rue_engine::store::Store;

use super::TempDir;

pub const OWNER: &str = "h";
pub const FAR: &str = "far";
pub const T0: u64 = 1_000_000;

pub fn record(name: &str, reach: &[&str]) -> HostRecord {
    HostRecord {
        name: name.into(),
        os: "freebsd".into(),
        reach: reach.iter().map(|s| s.to_string()).collect(),
        filesystem: true,
        stdin_preamble: true,
        artifact: None,
    }
}

pub fn host(name: &str, reach: &[&str]) -> Host {
    Host {
        record: record(name, reach),
        address: format!("10.0.0.{}", name.len()),
        scheduler: Some("cron".into()),
        rue_root: None,
        facts: BTreeMap::new(),
    }
}

/// The site: `h` reached by ssh; `far` reached by a transport no executor
/// serves (so a step on it is deferred).
pub fn site() -> Site {
    Site {
        hosts: vec![record(OWNER, &["ssh"]), record(FAR, &["carrier-pigeon"])],
        transports: vec!["ssh".into()],
        authenticators: vec![
            rue_core::model::Authenticator {
                id: "oncall".into(),
                human: true,
            },
            rue_core::model::Authenticator {
                id: "alice".into(),
                human: true,
            },
            rue_core::model::Authenticator {
                id: "driver".into(),
                human: false,
            },
        ],
        max_wait: Some(Duration::new(600)),
        scheduler_present: vec![OWNER.into()],
        secrets_deliver_to: vec!["requester".into(), "hold".into()],
    }
}

pub fn run(cmd: &str) -> Body {
    vec![Prim::Run(Run {
        cmd: vec![Part::Lit(cmd.into())],
        env: vec![],
        stdin: None,
    })]
}

/// An op whose do and undo are run bodies; its footprint is one owned file
/// named after the op so umbras are distinct.
pub fn op(id: &str) -> Op {
    let mut o = Op::new(
        id,
        vec![FootprintEntry::entry(Kind::Owned, &format!("file:/{id}"))],
    );
    o.do_ = run(&format!("do {id}"));
    o.undo = Undo::Computed {
        body: run(&format!("undo {id}")),
        undo_pre: vec![format!("file:/{id}")],
    };
    o
}

/// An op the artifact can undo: one owned file, restored by footprint,
/// undone on the target.
pub fn covered(id: &str) -> Op {
    let mut o = Op::new(
        id,
        vec![FootprintEntry::entry(Kind::Owned, &format!("file:/{id}"))],
    );
    o.do_ = vec![Prim::Write(rue_core::body::Write {
        fact: rue_core::body::FactRef {
            shape: format!("file:/{id}"),
            anchor: None,
        },
        content: rue_core::body::lit("x"),
    })];
    o.undo = Undo::Restore;
    o.undo_locus = rue_core::model::UndoLocus::Target;
    o
}

pub fn step(o: Op) -> Item {
    Item::Step(StepI::new(o))
}

pub fn temp_plan(id: &str, body: Vec<Item>) -> Plan {
    let mut p = Plan::new(id, OWNER, body);
    p.wane = Some(Duration::new(3600));
    p.renew_within = Some(Duration::new(600));
    p
}

pub fn perm_plan(id: &str, mut body: Vec<Item>) -> Plan {
    body.push(Item::Commit);
    Plan::new(id, OWNER, body)
}

pub fn ir(plan: Plan) -> PlanIr {
    PlanIr {
        ir_version: IR_VERSION,
        requester: "ops".into(),
        site: site(),
        plan,
    }
}

pub fn guard(name: &str, value: Tri) -> Guard {
    Guard::new(name, value)
}

/// A probe a plan may observe, declared the way a real `ssh()` host needs
/// it: with a `run` line. The fake answers it by name from `observe_as`,
/// whatever the line says.
///
/// Tests declare the probes their guards name rather than having `ir()`
/// declare them silently. The world's "ssh" is a fake that answers any
/// probe by name, which no real `ssh()` does, so a plan leaning on that
/// checked clean here and could never have run -- the gap E0608 closes.
/// Declaring them in each test keeps that visible instead of hiding it
/// from every engine test at once.
pub fn probe(name: &str) -> ProbeDecl {
    ProbeDecl {
        name: name.to_string(),
        locus: Locus::Target,
        body: vec![rue_core::body::Prim::Run(rue_core::body::Run {
            cmd: vec![rue_core::body::Part::Lit(format!("probe {name}"))],
            env: Vec::new(),
            stdin: None,
        })],
        produces: Vec::new(),
        static_: false,
        equivalence: "bytes".into(),
    }
}

pub fn hold(mut o: Op) -> Op {
    o.refusal = Refusal::Hold { via: None };
    o
}

pub fn on(mut o: Op, host: &str) -> Op {
    o.locus = Locus::Host(rue_core::model::HostRef::Static(host.into()));
    o
}

pub struct World {
    pub dir: TempDir,
    pub engine: Engine,
    pub ssh: FakeHandle,
    pub local: FakeHandle,
    pub sink: MemorySink,
    pub clock: Arc<FakeClock>,
    pub sched: FakeSchedulerHandle,
}

impl World {
    pub fn new(name: &str) -> World {
        let dir = TempDir::new(name);
        let store = Store::create(&dir.join("store")).unwrap();
        let sink = MemorySink::new("mem");
        let sinks: Vec<Box<dyn Sink>> = vec![Box::new(sink.clone())];
        let journal = Journal::open(&store, sinks, None).unwrap();
        let clock = Arc::new(FakeClock::at(rue_core::model::Instant::new(T0)));
        let ssh = FakeExecutor::new(LocusKind::Ssh).shared();
        let local = FakeExecutor::new(LocusKind::Local).shared();
        let execs: Vec<Box<dyn Executor>> = vec![Box::new(ssh.clone()), Box::new(local.clone())];
        let mut engine = Engine::open(
            store,
            journal,
            clock.clone(),
            execs,
            vec![host(OWNER, &["ssh"]), host(FAR, &["carrier-pigeon"])],
        )
        .unwrap();
        let sched = FakeSchedulerHandle::new();
        engine.add_scheduler(Box::new(sched.clone()));
        World {
            dir,
            engine,
            ssh,
            local,
            sink,
            clock,
            sched,
        }
    }

    /// Reopen the engine over the same store (a restart), executors, sink
    /// and scheduler carried over.
    pub fn restart(self) -> World {
        let World {
            dir,
            engine,
            ssh,
            local,
            sink,
            clock,
            sched,
        } = self;
        drop(engine);
        let store = Store::open(&dir.join("store")).unwrap();
        let sinks: Vec<Box<dyn Sink>> = vec![Box::new(sink.clone())];
        let journal = Journal::open(&store, sinks, None).unwrap();
        let execs: Vec<Box<dyn Executor>> = vec![Box::new(ssh.clone()), Box::new(local.clone())];
        let mut engine = Engine::open(
            store,
            journal,
            clock.clone(),
            execs,
            vec![host(OWNER, &["ssh"]), host(FAR, &["carrier-pigeon"])],
        )
        .unwrap();
        engine.add_scheduler(Box::new(sched.clone()));
        World {
            dir,
            engine,
            ssh,
            local,
            sink,
            clock,
            sched,
        }
    }

    pub fn advance(&self, secs: u64) {
        self.clock.advance(Duration::new(secs));
    }

    /// The commands the ssh fake ran, in order ("do a", "undo a", ...).
    pub fn commands(&self) -> Vec<String> {
        self.ssh
            .calls()
            .iter()
            .flat_map(|c| {
                c.body.iter().map(|p| match p {
                    rue_engine::executor::RPrim::Run { cmd, .. } => cmd.text.clone(),
                    other => format!("{other:?}"),
                })
            })
            .collect()
    }

    pub fn events(&self) -> Vec<String> {
        self.sink
            .events()
            .iter()
            .map(|e| format!("{e:?}"))
            .collect()
    }
}
