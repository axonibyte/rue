//! The shadow world: two hosts, two plans, and every binding a fake.
//!
//! Nothing here reaches inside the engine. An event is what an operator,
//! a target or the clock does, and what the invariants read afterwards is
//! what an operator could read too: the instance records, the journal,
//! the ledger, and the state of the hosts as their executor reports it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rue_core::body::{lit, FactRef, Prim, RegionSet, Run, Write};
use rue_core::ir::{PlanIr, IR_VERSION};
use rue_core::model::{
    Ack, Authenticator, Backstop, Cost, Drift, Duration, FootprintEntry, ForceName, HostRecord,
    Instant, Item, Kind, Op, Output, Plan, Refusal, Site, StepI, Trigger, Undo, UndoLocus,
};
use rue_engine::clock::{Clock, FakeClock};
use rue_engine::executor::{Executor, FakeExecutor, FakeHandle, LocusKind, Scripted};
use rue_engine::gates::FakeApprovalHandle;
use rue_engine::host::Host;
use rue_engine::journal::{Journal, MemorySink, Sink};
use rue_engine::lifecycle::{ApplyOptions, Engine};
use rue_engine::scheduler::FakeSchedulerHandle;
use rue_engine::secrets::FakeAcceptor;
use rue_engine::store::Store;

use crate::Event;

/// The host the plans act on.
pub const TARGET: &str = "fw-01";
/// The value the temporary plan's secret output carries: no sink, no
/// command line and no artifact may ever hold it (invariants 5 and 9).
pub const SECRET: &str = "sim-secret-must-never-appear";
/// The file the first two plans hold a region on, with different anchors
/// (invariant 12).
pub const SHARED: &str = "file:/etc/shared.conf";
/// The host the third plan's hook-executed step acts on: an appliance with
/// no filesystem, so its markers live in the controller's store and its
/// steps are the T4 shape (`undo_locus: :controller`).
pub const APPLIANCE: &str = "bmc-01";
/// The guest whose state the third plan's `modified` fact names: not a
/// file, and read by the probe that declares it `reads` (5.1, unit 2).
pub const GUEST_FACT: &str = "guest:state:g1";
/// The host no transport of this site serves: a step on it is deferred and
/// continued by `handoff-done`, which is T2's node-c (8.2).
pub const CONSOLE: &str = "node-c";

/// The names of the twenty invariants of docs/ROADMAP.md 10.3, in order.
pub const INVARIANTS: [&str; 20] = [
    "applied steps agree with the world",
    "owned and region footprints present",
    "the stuck set agrees",
    "the journal chain verifies",
    "no secret in any sink",
    "no wane fired during settle",
    "no reach op applied before its backstop armed",
    "engine and artifact undo to the same end state",
    "no secret in an argv, a history or an artifact",
    "no covered step ran before its artifact was installed",
    "no staged file survives an instance that is not applying",
    "no region clobbered while a sibling holds one",
    "the ledger holds exactly the reserving instances",
    "a proof for one scope satisfies no other",
    "reconciliation never removes an armed artifact",
    "no bounded state outlives its bound",
    "a region undo sees one manifest",
    "no act by an undeclared identity or outside its scope",
    "a permanent plan is never reverted by time",
    "a committed plan's backstop never fires",
];

fn record(name: &str, reach: &[&str]) -> HostRecord {
    HostRecord {
        name: name.into(),
        os: "freebsd".into(),
        reach: reach.iter().map(|s| s.to_string()).collect(),
        filesystem: true,
        stdin_preamble: true,
        artifact: None,
    }
}

/// A host of this world by name, for a caller that has only the name: the
/// invariants resolving a footprint shape need one, and every host here is
/// known by its name alone.
pub fn host_named(name: &str) -> Host {
    match name {
        APPLIANCE => host(APPLIANCE, &["api"]),
        CONSOLE => host(CONSOLE, &["console"]),
        "controller" => host("controller", &["local"]),
        other => host(other, &["ssh"]),
    }
}

fn host(name: &str, reach: &[&str]) -> Host {
    Host {
        record: record(name, reach),
        address: "10.0.0.1".into(),
        scheduler: Some("cron".into()),
        rue_root: None,
        facts: BTreeMap::new(),
    }
}

fn site() -> Site {
    let mut appliance = record(APPLIANCE, &["api"]);
    appliance.filesystem = false;
    let mut console = record(CONSOLE, &["console"]);
    console.filesystem = false;
    Site {
        hosts: vec![
            record(TARGET, &["ssh"]),
            record("controller", &["local"]),
            appliance,
            console,
        ],
        transports: vec!["ssh".into(), "local".into(), "api".into()],
        authenticators: vec![
            Authenticator {
                id: "oncall".into(),
                human: true,
            },
            Authenticator {
                id: "second".into(),
                human: true,
            },
        ],
        max_wait: Some(Duration::new(7_200)),
        scheduler_present: vec![TARGET.into()],
        secrets_deliver_to: vec!["hold".into()],
    }
}

fn run_body(cmd: &str) -> Vec<Prim> {
    vec![Prim::Run(Run {
        cmd: vec![rue_core::body::Part::Lit(cmd.into())],
        env: vec![],
        stdin: None,
    })]
}

fn write(shape: &str, content: &str) -> Prim {
    Prim::Write(Write {
        fact: FactRef {
            shape: shape.into(),
            anchor: None,
        },
        content: lit(content),
    })
}

fn region(shape: &str, anchor: &str, content: &str) -> Prim {
    Prim::RegionSet(RegionSet {
        fact: FactRef {
            shape: shape.into(),
            anchor: Some(anchor.into()),
        },
        content: lit(content),
    })
}

/// The temporary plan: a secret-bearing step on the target, a region on
/// the shared file, a `modified` fact under `:defer`, a `:target` undo
/// and a backstop that expires with its wane.
pub fn temporary() -> Plan {
    let mut first = Op::new(
        "issue",
        vec![FootprintEntry::entry(Kind::Owned, "file:/etc/issued")],
    );
    first.do_ = vec![write("file:/etc/issued", "issued")];
    first.undo = Undo::Restore;
    first.undo_locus = UndoLocus::Target;
    first.outputs = vec![Output {
        name: "token".into(),
        secret: true,
    }];
    first.reach = vec!["ssh".into()];

    let mut second = Op::new(
        "fence",
        vec![
            FootprintEntry::anchored(SHARED, "rue-sim-a"),
            FootprintEntry::entry(Kind::Modified, "file:/etc/kept"),
        ],
    );
    second.do_ = vec![
        region(SHARED, "rue-sim-a", "inside a"),
        write("file:/etc/kept", "changed"),
    ];
    second.undo = Undo::Restore;
    second.undo_locus = UndoLocus::Target;
    second.drift = Some(Drift::Defer);

    let mut p = Plan::new(
        "sim-temporary",
        TARGET,
        vec![
            Item::Step(StepI::new(first)),
            Item::Step(StepI::new(second)),
        ],
    );
    p.wane = Some(Duration::new(3_600));
    p.renew_within = Some(Duration::new(600));
    p.gate = Some(rue_core::model::PlanGate {
        expr: rue_core::model::GateExpr::Thresh {
            n: 2,
            factors: vec![
                rue_core::model::Factor::Auth {
                    id: "oncall".into(),
                    weight: 1,
                },
                rue_core::model::Factor::Auth {
                    id: "second".into(),
                    weight: 1,
                },
            ],
        },
        window: Some(Duration::new(7_200)),
        allow_zero_human: false,
    });
    p.backstop = Some(Backstop {
        triggers: vec![Trigger::After(Duration::new(3_600))],
        arm_before: 1,
    });
    p
}

/// The permanent plan: a knell, a region on the same shared file under
/// another anchor, a confirm and a commit, with an `unless_confirmed`
/// backstop.
pub fn permanent() -> Plan {
    let mut op = Op::new(
        "hold-open",
        vec![FootprintEntry::anchored(SHARED, "rue-sim-b")],
    );
    op.do_ = vec![region(SHARED, "rue-sim-b", "inside b")];
    op.undo = Undo::Restore;
    op.undo_locus = UndoLocus::Target;

    let mut knell = Op::new("cut", vec![]);
    knell.do_ = run_body("cut");
    knell.undo = Undo::NoUndo;
    knell.refusal = Refusal::Knell {
        guard: None,
        cost: Cost::NoCost("none".into()),
        ack: Ack::NoAck("declared".into()),
    };

    let mut p = Plan::new(
        "sim-permanent",
        TARGET,
        vec![
            Item::Step(StepI::new(op)),
            Item::Knell(StepI::new(knell)),
            Item::Confirm,
            Item::Commit,
        ],
    );
    p.backstop = Some(Backstop {
        triggers: vec![Trigger::UnlessConfirmed(Duration::new(1_800))],
        arm_before: 1,
    });
    p
}

/// The third plan, cut from the shapes T2 and T4 have (8.2, 8.4): a repeat
/// over a list, a `modified` fact that is not a file and is read by the
/// probe that declares it `reads`, a step on an appliance with no
/// filesystem (markers on the controller, `undo_locus: :controller`), a
/// staged file, a step gate, and a step no transport reaches, which defers
/// until `handoff-done`.
///
/// What it adds to the world is not more language but more orderings: an
/// iteration undone with its own variable, a deferred step outliving a
/// boot, a fact only a probe can read, and a proof made for the plan that
/// must not satisfy a step (invariants 11 and 14, which the first two
/// plans could not reach).
pub fn succession() -> Plan {
    // One iteration per guest, each owning a file of its own and reading
    // a fact that is not a file.
    let mut start = Op::new(
        "start-guest",
        vec![
            FootprintEntry::entry(Kind::Owned, "file:/etc/guest-{g}"),
            FootprintEntry::entry(Kind::Modified, "guest:state:{g}"),
        ],
    );
    start.do_ = vec![
        Prim::Stage(rue_core::body::Stage {
            name: "guest.conf".into(),
            content: lit("a staged file"),
            mode: 0o640,
        }),
        write("file:/etc/guest-{g}", "started"),
    ];
    start.undo = Undo::Restore;
    // A path bound at runtime cannot be baked into a target-side artifact
    // (E0202), which is exactly why T2's per-guest steps undo while the
    // engine lives.
    start.undo_locus = UndoLocus::Controller;

    // The appliance: no filesystem, so its markers live in the store and
    // its undo runs while the engine lives.
    let mut fence = Op::new(
        "fence-node",
        vec![FootprintEntry::entry(Kind::Modified, "power:node-a")],
    );
    fence.do_ = run_body("fence node-a");
    fence.undo = Undo::Computed {
        body: run_body("unfence node-a"),
        undo_pre: vec!["power:node-a".into()],
    };
    fence.undo_locus = UndoLocus::Controller;
    fence.locus = rue_core::model::Locus::Host(rue_core::model::HostRef::Static(APPLIANCE.into()));

    // The console: no transport of this site reaches it, so the step is
    // deferred and a `handoff-done` continues it.
    let mut heir = Op::new(
        "hand-over",
        vec![FootprintEntry::entry(Kind::Owned, "file:/etc/heir")],
    );
    heir.do_ = vec![write("file:/etc/heir", "heir")];
    heir.undo = Undo::Restore;
    heir.undo_locus = UndoLocus::Controller;
    heir.locus = rue_core::model::Locus::Host(rue_core::model::HostRef::Static(CONSOLE.into()));
    heir.handoff_done = Some("heir_seated".into());

    let mut gated = StepI::new(fence);
    gated.gate = Some(rue_core::model::GateExpr::Single(
        rue_core::model::Factor::Auth {
            id: "oncall".into(),
            weight: 1,
        },
    ));
    gated.window = Some(Duration::new(3_600));

    // A fenced block in the file the other two plans hold regions on,
    // under a third anchor. Two *live* region holders is what invariant 12
    // is about, and the permanent plan commits in one apply -- so until
    // this step existed, the check had no world it could fire in.
    let mut share = Op::new(
        "share-fence",
        vec![FootprintEntry::anchored(SHARED, "rue-sim-c")],
    );
    share.do_ = vec![region(SHARED, "rue-sim-c", "inside c")];
    share.undo = Undo::Restore;
    share.undo_locus = UndoLocus::Target;

    let mut p = Plan::new(
        "sim-succession",
        TARGET,
        vec![
            Item::Step(StepI::new(share)),
            Item::Repeat {
                form: rue_core::model::RepeatForm::Over {
                    list: "guests".into(),
                    max: 8,
                    set_valued: true,
                },
                var: "g".into(),
                body: vec![Item::Step(StepI::new(start))],
            },
            Item::Step(gated),
            Item::Step(StepI::new(heir)),
        ],
    );
    p.wane = Some(Duration::new(3_600));
    p.renew_within = Some(Duration::new(600));
    // The probe that reads the guest facts: over ssh, which reads files
    // alone, this is the only way `guest:state:{g}` is a fact at all.
    p.probes = vec![
        rue_core::model::ProbeDecl {
            name: "heir_seated".into(),
            locus: rue_core::model::Locus::Host(rue_core::model::HostRef::Static(CONSOLE.into())),
            body: run_body("seated"),
            produces: vec![],
            static_: false,
            equivalence: "bytes".into(),
            reads: None,
        },
        rue_core::model::ProbeDecl {
            name: "guest_state".into(),
            locus: rue_core::model::Locus::Target,
            body: run_body("jls -j {g} -h name"),
            produces: vec!["state".into()],
            static_: false,
            equivalence: "bytes".into(),
            reads: Some("guest:state:{g}".into()),
        },
    ];
    p
}

pub fn ir(plan: Plan) -> PlanIr {
    PlanIr {
        ir_version: IR_VERSION,
        requester: "requester".into(),
        site: site(),
        plan,
    }
}

/// A temporary directory removed with the world.
pub struct Scratch(pub PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "rue-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Scratch(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The world one run acts on.
pub struct Sim {
    pub dir: Scratch,
    pub engine: Engine,
    pub ssh: FakeHandle,
    /// The appliance's hook executor (no filesystem).
    pub api: FakeHandle,
    pub sched: FakeSchedulerHandle,
    pub approval: FakeApprovalHandle,
    pub secrets: FakeAcceptor,
    pub sink: MemorySink,
    pub clock: Arc<FakeClock>,
    /// The instances this run has requested, in order.
    pub instances: Vec<String>,
    /// What the sim knows it did, for the invariants that are about
    /// orderings rather than states.
    pub notes: Vec<String>,
    /// True while the engine is settling after a boot, as the sim sees it.
    pub booted_at: Option<Instant>,
}

impl Sim {
    pub fn new(name: &str) -> Sim {
        let dir = Scratch::new(name);
        let store = Store::create(&dir.0.join("store")).unwrap();
        let sink = MemorySink::new("mem");
        let sinks: Vec<Box<dyn Sink>> = vec![Box::new(sink.clone())];
        let journal = Journal::open(&store, sinks, None).unwrap();
        let clock = Arc::new(FakeClock::at(Instant::new(1_000_000)));
        let ssh = FakeExecutor::new(LocusKind::Ssh).shared();
        ssh.with(|f| {
            f.facts.insert(SHARED.to_string(), b"top\n".to_vec());
            f.facts
                .insert("file:/etc/kept".to_string(), b"kept\n".to_vec());
            f.clock = Some(Instant::new(1_000_000));
            // The guest states the third plan's probe reads: not files,
            // and answered by the probe alone (5.1, unit 2).
            f.observations.insert(
                "guest_state".to_string(),
                rue_engine::executor::Observation {
                    tri: Some(rue_core::model::Tri::Yes),
                    text: "running".into(),
                },
            );
        });
        let local = FakeExecutor::new(LocusKind::Local).shared();
        // The appliance's executor: a hook, which knows its facts by name
        // and has no filesystem, so the third plan's step there keeps its
        // markers in the store and undoes while the engine lives.
        let api = FakeExecutor::new(LocusKind::Hook("api".into())).shared();
        api.with(|f| {
            f.caps.filesystem = false;
            f.facts.insert("power:node-a".to_string(), b"on\n".to_vec());
        });
        let execs: Vec<Box<dyn Executor>> = vec![
            Box::new(ssh.clone()),
            Box::new(local),
            Box::new(api.clone()),
        ];
        let mut engine = Engine::open(
            store,
            journal,
            clock.clone(),
            execs,
            vec![
                host(TARGET, &["ssh"]),
                host(APPLIANCE, &["api"]),
                host(CONSOLE, &["console"]),
            ],
        )
        .unwrap();
        let sched = FakeSchedulerHandle::new();
        engine.add_scheduler(Box::new(sched.clone()));
        let approval = FakeApprovalHandle::new(site().authenticators.clone());
        engine.set_approval(Box::new(approval.clone()));
        let secrets = FakeAcceptor::new("hold", true);
        engine.add_acceptor(Box::new(secrets.clone()));
        Sim {
            dir,
            engine,
            ssh,
            api,
            sched,
            approval,
            secrets,
            sink,
            clock,
            instances: Vec::new(),
            notes: Vec::new(),
            booted_at: None,
        }
    }

    fn opts() -> ApplyOptions {
        ApplyOptions {
            by: "requester".into(),
            ..ApplyOptions::default()
        }
    }

    /// The instance the verbs act on: the last one requested.
    pub fn current(&self) -> Option<String> {
        self.instances.last().cloned()
    }

    /// The facts of a host as its executor holds them, or `None` for a
    /// host no executor of this world reaches -- whose world the engine
    /// never touched and cannot read.
    pub fn facts_on(&self, host: &str) -> Option<BTreeMap<String, Vec<u8>>> {
        match host {
            TARGET => Some(self.ssh.with(|f| f.facts.clone())),
            APPLIANCE => Some(self.api.with(|f| f.facts.clone())),
            _ => None,
        }
    }

    /// Every instance record the store holds.
    pub fn records(&self) -> Vec<rue_engine::lifecycle::InstanceRecord> {
        self.engine.instances().unwrap_or_default()
    }

    /// One event against the world. A verb the state does not admit is a
    /// refusal, which is an outcome and not a violation.
    pub fn apply(&mut self, e: Event) {
        match e {
            Event::ApplyTemporary => self.request(temporary()),
            Event::ApplyPermanent => self.request(permanent()),
            Event::ApplySuccession => self.request(succession()),
            Event::HandoffDone => {
                // The step the instance is actually deferred at; a handoff
                // reported for another step is a refusal, and the sim
                // reports the one an operator would be told to report.
                if let Some(id) = self.current() {
                    let at = self
                        .records()
                        .into_iter()
                        .find(|r| r.id == id)
                        .and_then(|r| r.deferred.map(|d| d.step));
                    if let Some(step) = at {
                        let _ = self.engine.handoff_done(&id, step, "requester");
                    }
                }
            }
            Event::ApproveStep(n) => {
                if let Some(id) = self.current() {
                    let _ = self.engine.approve_proof(
                        &id,
                        rue_core::journal::Scope::Step(n as u32),
                        "oncall",
                        "token",
                        "requester",
                    );
                }
            }
            Event::Approve(which) => {
                let who = if which == 0 { "oncall" } else { "second" };
                if let Some(id) = self.current() {
                    let _ = self.engine.approve_proof(
                        &id,
                        rue_core::journal::Scope::Plan,
                        who,
                        "token",
                        "requester",
                    );
                }
            }
            Event::Tick(s) => {
                self.clock.advance(Duration::new(s));
                let now = self.clock.now();
                self.ssh.with(|f| f.clock = Some(now));
            }
            Event::Reap => {
                let _ = self.engine.reap();
            }
            Event::Reboot => {
                self.booted_at = Some(self.clock.now());
                let _ = self.engine.boot();
            }
            Event::Recant => {
                if let Some(id) = self.current() {
                    let _ = self.engine.recant(&id, &[]);
                }
            }
            Event::RecantForcingDrift => {
                if let Some(id) = self.current() {
                    let _ = self.engine.recant(&id, &[ForceName::Drift]);
                }
            }
            Event::EditFact => {
                // Someone edits a fact the plan owns, behind the engine.
                self.ssh.with(|f| {
                    f.facts
                        .insert("file:/etc/kept".to_string(), b"edited by hand\n".to_vec());
                });
                self.notes.push("edited file:/etc/kept".into());
            }
            Event::Fire => self.fire(),
            Event::LoseSchedulerEntry => {
                self.sched.with(|f| f.entries.clear());
            }
            Event::Confirm => {
                if let Some(id) = self.current() {
                    let _ = self.engine.confirm(&id);
                }
            }
            Event::Commit => {
                if let Some(id) = self.current() {
                    let _ = self.engine.commit(&id, "requester", "the sim said so");
                }
            }
            Event::Abandon => {
                if let Some(id) = self.current() {
                    let _ = self.engine.abandon(&id, "requester", "the sim said so");
                }
            }
            Event::DropRead => {
                // The next read of the fact both plans touch fails. What
                // must not happen is the engine reading the failure as
                // "the fact is not there" and undoing on that (R0205).
                self.ssh
                    .with(|f| f.failing_reads.insert("file:/etc/kept".to_string(), 0));
                self.notes.push("reads of file:/etc/kept drop".into());
            }
            Event::BreakExecutor => {
                self.ssh.with(|f| {
                    f.script
                        .push_back(Scripted::Fail("the sim broke it".into()))
                });
            }
        }
    }

    /// The artifact fires on its target.
    ///
    /// The sim applies the artifact's rule (docs/DESIGN.md: undo only
    /// steps whose marker is present, in reverse, each step's drift
    /// policy as the engine's, removing the marker after) to the shadow
    /// world, rather than executing the rendered script. What an executed
    /// artifact does is proven where a real one runs: `render`'s own
    /// execution tests under `sh` and Python, and the end-to-end harness
    /// where a real cron fires a real artifact. What the simulation is
    /// for is the orderings around it -- a firing that races a recant, a
    /// boot, a wane -- and for those the rule is enough.
    fn fire(&mut self) {
        let Some(id) = self.current() else { return };
        let Some(rec) = self.records().into_iter().find(|r| r.id == id) else {
            return;
        };
        if !rec.backstop.as_ref().is_some_and(|b| b.armed && !b.fired) {
            return;
        }
        let covered: Vec<u32> = rue_core::backstop::coverage(rec.plan())
            .map(|c| c.covered)
            .unwrap_or_default();
        let mut drifted = false;
        for n in covered.into_iter().rev() {
            let key = |rel: &str| (TARGET.to_string(), id.clone(), rel.to_string());
            let markers = self
                .ssh
                .with(|f| f.files.get(&key(&format!("markers/{n}"))).cloned());
            let Some(markers) = markers else { continue };
            let markers = rue_engine::footprint::parse_markers(&String::from_utf8_lossy(&markers));
            let Some(op) = rec.op_at(n) else { continue };
            let policy = op
                .effective_drift()
                .unwrap_or(rue_core::model::Drift::Clobber);
            let mut held = false;
            // `file_facts` and not `observed_facts`, deliberately: this
            // models the RENDERED ARTIFACT, which has no executor and can
            // only look at files, and it reads `markers/<n>` -- which the
            // engine writes from the file facts alone for that reason. The
            // engine decides drift over every fact it can read back; the
            // artifact keeps the narrower half of 5.2's rule, and 5.2 says
            // so. Widening this to match the engine would have the shadow
            // world compare shapes against markers that do not carry them.
            for (k, e, path) in rue_engine::footprint::file_facts(&op.footprint) {
                let now = self.ssh.with(|f| f.facts.get(&e.shape).cloned());
                let digest = rue_engine::footprint::digest_of(now.as_deref());
                let recorded = markers
                    .iter()
                    .find(|m| m.path == path)
                    .map(|m| m.digest.clone());
                let changed = recorded.as_ref().is_some_and(|r| *r != digest);
                match e.kind {
                    Kind::Region => {
                        let text = now
                            .as_deref()
                            .map(|b| String::from_utf8_lossy(b).into_owned())
                            .unwrap_or_default();
                        let anchor = e.anchor.clone().unwrap_or_default();
                        match rue_engine::region::strip(&text, &anchor) {
                            Some(stripped) => self.ssh.with(|f| {
                                f.facts.insert(e.shape.clone(), stripped.into_bytes());
                            }),
                            // Damaged markers: the whole file from the
                            // snapshot, which is the artifact's fallback.
                            None => {
                                let snap = self.ssh.with(|f| {
                                    f.files.get(&key(&format!("snapshots/{n}/{k}"))).cloned()
                                });
                                if let Some(snap) = snap {
                                    self.ssh.with(|f| {
                                        f.facts.insert(e.shape.clone(), snap);
                                    });
                                }
                            }
                        }
                    }
                    Kind::Owned => {
                        if changed && policy == rue_core::model::Drift::Defer {
                            held = true;
                        } else {
                            self.ssh.with(|f| {
                                f.facts.remove(&e.shape);
                            });
                        }
                    }
                    Kind::Modified => {
                        if changed && policy == rue_core::model::Drift::Defer {
                            held = true;
                        } else {
                            let snap = self.ssh.with(|f| {
                                f.files.get(&key(&format!("snapshots/{n}/{k}"))).cloned()
                            });
                            if let Some(snap) = snap {
                                self.ssh.with(|f| {
                                    f.facts.insert(e.shape.clone(), snap);
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            if held {
                drifted = true;
                continue;
            }
            self.ssh.with(|f| {
                f.files.remove(&key(&format!("markers/{n}")));
            });
        }
        self.ssh.with(|f| {
            f.files.insert(
                (TARGET.to_string(), id.clone(), "fired".to_string()),
                Vec::new(),
            );
            if drifted {
                f.files.insert(
                    (TARGET.to_string(), id.clone(), "drift".to_string()),
                    Vec::new(),
                );
            }
        });
        self.notes.push(format!("{id} fired"));
    }

    fn request(&mut self, plan: Plan) {
        // The list the third plan repeats over, as a plan parameter.
        let mut params = BTreeMap::new();
        if plan.id == "sim-succession" {
            params.insert("guests".to_string(), "g1,g2".to_string());
        }
        // The secret the first step produces, for the run that reaches it.
        let mut outputs = BTreeMap::new();
        outputs.insert("token".to_string(), SECRET.to_string());
        self.ssh.with(|f| {
            f.script
                .push_back(Scripted::Ok(rue_engine::executor::Output {
                    stdout: String::new(),
                    outputs,
                }))
        });
        let id = plan.id.clone();
        // A refusal is an outcome, not a violation -- but a refusal nobody
        // can see is a silent no-op, and an event list full of those is a
        // sweep that looks twice the size it is.
        match self.engine.apply(ir(plan), params, Sim::opts()) {
            Ok(out) => {
                if !self.instances.contains(&out.id) {
                    self.instances.push(out.id);
                }
            }
            Err(e) => self.notes.push(format!("{id} refused: {e}")),
        }
    }
}
