//! The lifecycle, docs/ROADMAP.md 5.9, 7.1 and 7.8: an instance from check
//! to a terminal state, over the store, the journal, the clock and the
//! executors.
//!
//! The state machine is core's (`rue_core::states::transition`); this
//! module derives the events. An event comes from a verb (`apply`,
//! `recant`, `renew`, `confirm`, `commit`, `resume`, `handoff-done`,
//! `abandon`, `cancel`), from a step's outcome (a refusal, a guard, a
//! deferred host, the last step, `commit()`), or from the reap pass
//! observing time (wane, an approval window, a wait's bound). Every
//! transition is journaled; every `now` is the clock's.
//!
//! Write-ahead: the `Applying{step, undo_line}` entry is recorded and
//! acknowledged by every sink, and the instance persisted in `Applying`,
//! before a step's `do` runs; a crash after that point is demoted to
//! `Reverting` at boot and the step's undo runs whether or not its `do`
//! finished (section 5.9: a failed step is itself reverted; it may be
//! half-applied).
//!
//! Progress is a set, not a cursor: the applied leaves (with their repeat
//! iteration) and the arm each `when` chose are persisted, and the plan is
//! walked from its start every time, skipping what is done. A walk after a
//! crash therefore makes the same choices and resumes at the same leaf.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use rue_core::algebra::{numbered, op_of};
use rue_core::body::Body;
use rue_core::check;
use rue_core::explain::undo_line;
use rue_core::intent::{effective_wane, infer_intent, Intent};
use rue_core::interference::{maywrite, step_facts, writes};
use rue_core::ir::PlanIr;
use rue_core::journal::{Event as J, Scope};
use rue_core::ledger::{Instance as Held, Ledger, LedgerCode};
use rue_core::model::{
    Ack, Drift, Duration, FootprintEntry, ForceName, Guard, HostRef, Instant, Item, Kind, Locus,
    Mode, OnLapse, Op, Plan, Refusal, RepeatForm, StepI, Tri, Undo, UndoLocus,
};
use rue_core::request::hash_json;
use rue_core::states::{self, Ctx, Event as E, Outcome as Verdict_, RCode, State};
use rue_core::verdict::{Status, Verdict};
use serde::{Deserialize, Serialize};

use crate::backstop::{BackstopState, Disarm, DEFAULT_SKEW_TOLERANCE};
use crate::clock::Clock;
use crate::executor::{BootstrapState, ExecCaps, Executor, Observation, ProbeRun, RPrim, Resolved};
use crate::footprint::{self, Decision, Marker, Watched};
use crate::gates::{hex, nonce, Approval, Proof};
use crate::host::Host;
use crate::journal::{About, Journal, JournalError};
use crate::notify::{Level, Notify};
use crate::region;
use crate::resolve::{resolve_body, Env};
use crate::scheduler::Scheduler;
use crate::secrets::Acceptor;
use crate::store::{Store, StoreError};

// ---------------------------------------------------------------------------
// The persisted record

fn state_name(s: State) -> String {
    s.to_string()
}

fn parse_state(s: &str) -> Option<State> {
    states::ALL_STATES
        .iter()
        .copied()
        .find(|st| st.to_string() == s)
}

mod state_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(s: &State, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&state_name(*s))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<State, D::Error> {
        let text = String::deserialize(de)?;
        parse_state(&text).ok_or_else(|| serde::de::Error::custom(format!("no such state: {text}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedStep {
    pub step: u32,
    pub iteration: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wait {
    pub step: u32,
    pub reason: String,
    pub since: Instant,
    /// The wait's own bound (window or max_wait); wane still wins.
    pub bound: Option<Instant>,
    /// The guard being waited on, when the wait is an unknown guard.
    pub guard: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeferredAt {
    pub step: u32,
    pub handoff: String,
}

/// A file `stage()`d for a step, removed after it (7.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Staged {
    pub host: String,
    pub step: u32,
    pub name: String,
}

/// One instance, as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceRecord {
    pub id: String,
    pub ir: PlanIr,
    pub params: BTreeMap<String, String>,
    #[serde(with = "state_serde")]
    pub state: State,
    pub permanent: bool,
    pub rehearsal: bool,
    pub requested_at: Option<Instant>,
    pub approved_at: Option<Instant>,
    /// The wane deadline of a temporary plan, anchored at approval or the
    /// last renewal.
    pub deadline: Option<Instant>,
    pub approval_deadline: Option<Instant>,
    pub applied: Vec<AppliedStep>,
    /// The arm each `when` chose, by the leaf number of its first leaf.
    pub choices: BTreeMap<String, bool>,
    pub outputs: BTreeMap<String, String>,
    /// Controller-side snapshots of file facts before `do`, by `step/k`
    /// (unit C moves those of run-capable hosts to the instance directory).
    pub snapshots: BTreeMap<String, String>,
    pub waiting: Option<Wait>,
    pub held_at: Option<u32>,
    pub deferred: Option<DeferredAt>,
    pub stuck: Vec<u32>,
    pub drift_held: Vec<u32>,
    /// The markers of every applied step's file facts as `do` left them
    /// (the controller's copy of `markers/<n>`), by step.
    #[serde(default)]
    pub markers: BTreeMap<String, Vec<Marker>>,
    /// Hosts on which this instance has an instance directory.
    #[serde(default)]
    pub dirs: Vec<String>,
    #[serde(default)]
    pub staged: Vec<Staged>,
    /// `recant --force=drift`: drift under `:defer` is clobbered.
    #[serde(default)]
    pub force_drift: bool,
    /// The `:target` backstop, once the engine has put it on its host.
    #[serde(default)]
    pub backstop: Option<BackstopState>,
    /// The request nonce, hex (5.11): core draws no randomness.
    #[serde(default)]
    pub nonce: String,
    /// The host contract frozen at the request, hex; a change is R0301.
    #[serde(default)]
    pub host_contract: String,
    /// Proofs accepted, by scope.
    #[serde(default)]
    pub proofs: Vec<Proof>,
    /// A secret this instance produced that no acceptor took: exit 7.
    #[serde(default)]
    pub secret_undelivered: bool,
    /// The step whose `do` is running right now, written before it starts
    /// and cleared when it ends. An engine that dies here left a step
    /// half-done, and the write-ahead entry is what says how to undo it
    /// (5.9, 7.8).
    #[serde(default)]
    pub attempting: Option<AppliedStep>,
    /// Knells acknowledged up front (`--ack`), by step.
    pub acks: Vec<u32>,
    /// Guard names forced by the request or a `recant --force`.
    pub forced: Vec<String>,
    pub ledger_ids: Vec<String>,
    pub refusal: Option<String>,
    pub closed_reason: Option<String>,
}

impl InstanceRecord {
    pub fn plan(&self) -> &Plan {
        &self.ir.plan
    }

    pub fn intent(&self) -> Intent {
        if self.permanent {
            Intent::Permanent
        } else {
            Intent::Temporary
        }
    }

    pub fn is_applied(&self, step: u32, iteration: u32) -> bool {
        self.applied
            .iter()
            .any(|a| a.step == step && a.iteration == iteration)
    }

    /// The context the state machine reads (section 5.9).
    pub fn ctx(&self) -> Ctx {
        let earlier_hold = self.applied.iter().any(|a| {
            self.op_at(a.step)
                .is_some_and(|o| matches!(o.refusal, Refusal::Hold { .. }))
        });
        let on_lapse = self
            .waiting
            .as_ref()
            .and_then(|w| self.step_at(w.step))
            .map(|s| s.on_lapse)
            .unwrap_or(OnLapse::Revert);
        Ctx {
            intent: self.intent(),
            mode: self.plan().mode,
            earlier_hold,
            on_lapse,
        }
    }

    pub fn op_at(&self, step: u32) -> Option<&Op> {
        numbered(&self.plan().body)
            .into_iter()
            .find(|(n, _)| *n == step)
            .and_then(|(_, it)| op_of(it))
    }

    pub fn step_at(&self, step: u32) -> Option<&StepI> {
        numbered(&self.plan().body)
            .into_iter()
            .find(|(n, _)| *n == step)
            .and_then(|(_, it)| rue_core::algebra::step_of(it))
    }

    pub fn about(&self) -> About {
        About {
            plan: self.plan().id.clone(),
            instance: self.id.clone(),
            host: self.plan().owner.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Outcomes and errors

/// What a verb reports: the state reached and the exit code of 6.8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub id: String,
    pub state: State,
    pub exit: u8,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The check refused the plan; the verdict is the reason.
    Refused(Box<Verdict>),
    /// R0101 (exit 75) or R0203 at request.
    Ledger(LedgerCode, String),
    /// R0102, R0103: the verb is not admitted here.
    NotAdmitted(RCode, String),
    NoSuchInstance(String),
    /// A verb that has no meaning in the instance's state.
    WrongState {
        state: State,
        verb: String,
    },
    Journal(JournalError),
    Store(StoreError),
    /// A refusal the world made, naming its own R-code (R0401, R0403,
    /// R0404, R0405, R0406): the verb was admitted, the target refused.
    Runtime(String),
    /// A malformed plan the checker should have refused.
    Internal(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Refused(v) => write!(f, "refused: {}", rue_core::prose::prose(v)),
            EngineError::Ledger(c, m) => write!(f, "{c:?}: {m}"),
            EngineError::NotAdmitted(c, m) => write!(f, "{c:?}: {m}"),
            EngineError::NoSuchInstance(id) => write!(f, "no such instance: {id}"),
            EngineError::WrongState { state, verb } => {
                write!(f, "{verb} has no meaning in state {state}")
            }
            EngineError::Journal(e) => write!(f, "{e}"),
            EngineError::Store(e) => write!(f, "{e}"),
            EngineError::Runtime(s) => write!(f, "{s}"),
            EngineError::Internal(s) => write!(f, "internal: {s}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<JournalError> for EngineError {
    fn from(e: JournalError) -> EngineError {
        EngineError::Journal(e)
    }
}

impl From<StoreError> for EngineError {
    fn from(e: StoreError) -> EngineError {
        EngineError::Store(e)
    }
}

/// The most a snapshot may be, in bytes: the number the verdict states
/// (`snapshot_cap`), enforced where the snapshot is taken (R0204).
pub const SNAPSHOT_CAP: u64 = 1_048_576;

/// The exit code of section 6.8 for a state.
/// `refused` is whether the instance closed *because* something refused
/// it: a plan that reverted after a refusal is exit 1, and one an
/// operator recanted, cancelled or abandoned, or that waned and reverted
/// cleanly, is exit 0. Both end `Closed`, and only the reason tells them
/// apart (section 6.8: `0` applied or ok, `1` refused).
pub fn exit_of(state: State, refused: bool, secret_undelivered: bool) -> u8 {
    match state {
        State::Held => 3,
        State::Stuck => 4,
        State::Deferred => 5,
        State::Pending | State::Waiting => 6,
        State::DriftHeld => 8,
        State::Closed if refused => 1,
        State::Closed => 0,
        _ if secret_undelivered => 7,
        _ => 0,
    }
}

#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    pub rehearsal: bool,
    pub acks: Vec<u32>,
    pub forced: Vec<String>,
    pub mode: Option<Mode>,
    /// Who is acting, for the journal.
    pub by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReapReport {
    /// What the pass did, one line per action.
    pub actions: Vec<String>,
    /// Unbounded states whose notification is re-sent: (instance, state).
    pub notify: Vec<(String, State)>,
    /// True when the pass skipped wane and retries because the engine is
    /// settling after boot.
    pub settling: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootReport {
    pub demoted: Vec<String>,
    pub reestablished: Vec<(String, u32)>,
    pub lost: Vec<(String, u32)>,
    pub reobserved: Vec<(String, String)>,
    pub migrated: Option<(u32, u32)>,
    /// Instance directories the store does not know that hold an armed,
    /// unfired artifact: left where they are (7.7).
    pub orphaned: Vec<(String, String)>,
    /// Directories with no artifact or a fired marker, removed.
    pub reclaimed: Vec<(String, String)>,
    /// Held secrets a restart dropped: none survives it (5.13).
    pub secrets_dropped: usize,
    /// Steps whose `do` the restart interrupted, undone on the way back:
    /// (instance, step).
    pub interrupted: Vec<(String, u32)>,
}

// ---------------------------------------------------------------------------
// The engine

/// How a step's walk ended.
enum Flow {
    Continue,
    /// The instance left `Applying`; stop walking.
    Stop,
}

/// One transition the machine made (or refused), as the engine observed
/// it: what `rue status` reports as history and what the tier-4 table test
/// reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub id: String,
    pub from: State,
    pub event: E,
    pub outcome: Verdict_,
}

pub struct Engine {
    pub(crate) store: Store,
    pub(crate) journal: Journal,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) executors: Vec<Box<dyn Executor>>,
    /// The `backstop scheduler` bindings, by the name a host declares.
    pub(crate) schedulers: Vec<Box<dyn Scheduler>>,
    /// The `approval via:` binding, if the site bound one.
    pub(crate) approval: Option<Box<dyn Approval>>,
    /// `secrets deliver_to:`, in the order the site declares them.
    pub(crate) acceptors: Vec<Box<dyn Acceptor>>,
    /// `notify via:`, where an unbounded state says so each reap pass.
    pub(crate) notify: Option<Box<dyn Notify>>,
    pub(crate) hosts: BTreeMap<String, Host>,
    pub(crate) controller: Host,
    pub(crate) ledger: Ledger,
    /// The site's `skew_tolerance` (7.7); 120s where the site declares none.
    pub(crate) skew_tolerance: Duration,
    pub(crate) settling: bool,
    pub(crate) trace: Vec<Transition>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Engine(store {}, {} executors, {} hosts, settling {})",
            self.store.root().display(),
            self.executors.len(),
            self.hosts.len(),
            self.settling
        )
    }
}

/// One host in `rue doctor`'s report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDoctor {
    pub name: String,
    /// The transport of the executor that reaches it, or none.
    pub executor: Option<String>,
    pub bootstrap: Option<BootstrapState>,
    pub scheduler: Option<String>,
    /// The site bound a `backstop scheduler` by the name this host
    /// declares. A host that names one the site never bound is R0401 when
    /// a plan puts a backstop on it.
    pub scheduler_bound: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub hosts: Vec<HostDoctor>,
    /// Instance directories the store does not know that hold an armed
    /// artifact: left in place by reconciliation, reclaimed by hand.
    #[serde(default)]
    pub orphans: Vec<(String, String)>,
    pub sinks: Vec<String>,
    pub signed: bool,
    pub settling: bool,
    pub instances: usize,
}

impl DoctorReport {
    /// Every host reached and bootstrapped, nothing settling.
    pub fn healthy(&self) -> bool {
        !self.settling
            && self
                .hosts
                .iter()
                .all(|h| h.executor.is_some() && h.bootstrap.as_ref().is_some_and(|b| b.ready()))
    }
}

/// The commands `rue bootstrap` prints for what a target lacks (7.7),
/// per OS family; rue never runs them.
pub fn bootstrap_commands(os: &str, root: &str, state: &BootstrapState) -> Vec<String> {
    let mut v = Vec::new();
    if os == "windows" {
        if !state.group {
            v.push("New-LocalGroup -Name rue".into());
        }
        if !state.rue_root || !state.instances_dir || !state.lock || !state.modes_ok {
            v.push(format!(
                "New-Item -ItemType Directory -Force {root}\\instances"
            ));
            v.push(format!("New-Item -ItemType File -Force {root}\\lock"));
            v.push(format!(
                "icacls {root}\\instances /grant rue:(OI)(CI)M; icacls {root}\\lock /grant rue:M"
            ));
        }
        return v;
    }
    if !state.group {
        v.push(match os {
            "freebsd" | "dragonfly" => "pw groupadd rue".to_string(),
            _ => "groupadd rue".to_string(),
        });
    }
    if !state.rue_root {
        v.push(format!("install -d -o root -m 0755 {root}"));
    }
    if !state.instances_dir || !state.modes_ok {
        v.push(format!(
            "install -d -o root -g rue -m 2770 {root}/instances"
        ));
    }
    if !state.lock || !state.modes_ok {
        v.push(format!(
            "install -o root -g rue -m 0664 /dev/null {root}/lock"
        ));
    }
    v
}

/// The controller as a host: where `:controller` steps and probes run.
pub fn controller_host() -> Host {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "freebsd" => "freebsd",
        _ => "linux",
    };
    Host {
        record: rue_core::model::HostRecord {
            name: "controller".into(),
            os: os.into(),
            reach: vec!["local".into()],
            filesystem: true,
            stdin_preamble: true,
            artifact: None,
        },
        address: "127.0.0.1".into(),
        scheduler: None,
        rue_root: None,
        facts: BTreeMap::new(),
    }
}

impl Engine {
    pub fn open(
        store: Store,
        journal: Journal,
        clock: Arc<dyn Clock>,
        executors: Vec<Box<dyn Executor>>,
        hosts: Vec<Host>,
    ) -> Result<Engine, EngineError> {
        let ledger = store.read_ledger()?;
        let settling = store
            .read_meta("settle")
            .map(|m| m.get("settling").map(String::as_str) == Some("true"))
            .unwrap_or(false);
        Ok(Engine {
            store,
            journal,
            clock,
            executors,
            hosts: hosts
                .into_iter()
                .map(|h| (h.name().to_string(), h))
                .collect(),
            controller: controller_host(),
            ledger,
            schedulers: Vec::new(),
            approval: None,
            acceptors: Vec::new(),
            notify: None,
            skew_tolerance: DEFAULT_SKEW_TOLERANCE,
            settling,
            trace: Vec::new(),
        })
    }

    /// Every transition since the engine opened, in order.
    pub fn trace(&self) -> &[Transition] {
        &self.trace
    }

    /// A site-level journal entry (an operator connected, a hook registered):
    /// about no plan and no instance.
    pub fn journal_site_event(&mut self, ev: J) -> Result<rue_core::journal::Entry, EngineError> {
        let at = self.clock.now();
        let about = About {
            plan: String::new(),
            instance: String::new(),
            host: String::new(),
        };
        Ok(self
            .journal
            .record(&self.store, at, &about, ev, Vec::new())?)
    }

    /// Replace the host inventory (a hook inventory listed anew).
    pub fn set_hosts(&mut self, hosts: Vec<Host>) {
        self.hosts = hosts
            .into_iter()
            .map(|h| (h.name().to_string(), h))
            .collect();
    }

    /// The `approval via:` binding: what renders a challenge over a
    /// request digest and verifies the proofs that come back.
    pub fn set_approval(&mut self, a: Box<dyn Approval>) {
        self.approval = Some(a);
    }

    /// The `notify via:` binding.
    pub fn set_notify(&mut self, n: Box<dyn Notify>) {
        self.notify = Some(n);
    }

    /// A `secrets deliver_to:` acceptor, appended in the site's order.
    pub fn add_acceptor(&mut self, a: Box<dyn Acceptor>) {
        self.acceptors.push(a);
    }

    /// A `backstop scheduler` binding; a host names one in its record.
    pub fn add_scheduler(&mut self, s: Box<dyn Scheduler>) {
        self.schedulers.push(s);
    }

    /// The site's declared skew tolerance for the arm-time clock probe.
    pub fn set_skew_tolerance(&mut self, d: Duration) {
        self.skew_tolerance = d;
    }

    pub fn add_executor(&mut self, e: Box<dyn Executor>) {
        self.executors.push(e);
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn settling(&self) -> bool {
        self.settling
    }

    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    pub fn status(&self, id: &str) -> Result<Option<InstanceRecord>, EngineError> {
        Ok(self.store.read_instance(id)?)
    }

    pub fn instances(&self) -> Result<Vec<InstanceRecord>, EngineError> {
        let mut v = Vec::new();
        for id in self.store.instance_ids()? {
            if let Some(r) = self.store.read_instance(&id)? {
                v.push(r);
            }
        }
        Ok(v)
    }

    pub(crate) fn load(&self, id: &str) -> Result<InstanceRecord, EngineError> {
        self.store
            .read_instance(id)?
            .ok_or_else(|| EngineError::NoSuchInstance(id.to_string()))
    }

    pub(crate) fn persist(&self, rec: &InstanceRecord) -> Result<(), EngineError> {
        Ok(self.store.write_instance(&rec.id, rec)?)
    }

    pub(crate) fn log(&mut self, rec: &InstanceRecord, ev: J) -> Result<(), EngineError> {
        let at = self.clock.now();
        self.journal
            .record(&self.store, at, &rec.about(), ev, Vec::new())?;
        Ok(())
    }

    fn set_settling(&mut self, on: bool) -> Result<(), EngineError> {
        self.settling = on;
        let mut m = BTreeMap::new();
        m.insert("settling".to_string(), on.to_string());
        Ok(self.store.write_meta("settle", &m)?)
    }

    pub(crate) fn outcome(&self, rec: &InstanceRecord) -> Outcome {
        let exit = exit_of(rec.state, rec.refusal.is_some(), rec.secret_undelivered);
        let line = match rec.state {
            State::Closed => format!(
                "{}: closed ({})",
                rec.id,
                rec.closed_reason.as_deref().unwrap_or("no reason recorded")
            ),
            State::Waiting => format!(
                "{}: waiting at step {} ({})",
                rec.id,
                rec.waiting.as_ref().map(|w| w.step).unwrap_or(0),
                rec.waiting
                    .as_ref()
                    .map(|w| w.reason.as_str())
                    .unwrap_or("")
            ),
            State::Pending => format!("{}: pending approval", rec.id),
            State::Held => format!("{}: held at step {}", rec.id, rec.held_at.unwrap_or(0)),
            State::Deferred => format!(
                "{}: deferred at step {}",
                rec.id,
                rec.deferred.as_ref().map(|d| d.step).unwrap_or(0)
            ),
            State::Stuck => format!("{}: stuck at steps {:?}", rec.id, rec.stuck),
            State::DriftHeld => format!(
                "{}: R0202: drift-held at steps {:?}",
                rec.id, rec.drift_held
            ),
            State::Applied if rec.rehearsal => {
                format!("{}: applied (rehearsal: no reservation)", rec.id)
            }
            State::Applied | State::Committed if rec.secret_undelivered => {
                format!("{}: applied; secret undelivered", rec.id)
            }
            s => format!("{}: {}", rec.id, s.to_string().to_lowercase()),
        };
        Outcome {
            id: rec.id.clone(),
            state: rec.state,
            exit,
            line,
        }
    }

    // --- executors and hosts ------------------------------------------------

    pub(crate) fn host_of(&self, name: &str) -> Option<Host> {
        if name == "controller" {
            return Some(self.controller.clone());
        }
        self.hosts.get(name).cloned()
    }

    /// The host a step acts on; `Err` names the output a bound host comes
    /// from (the step is deferred).
    pub(crate) fn step_host(&self, rec: &InstanceRecord, op: &Op) -> Result<Host, String> {
        let name = match &op.locus {
            Locus::Controller => return Ok(self.controller.clone()),
            Locus::Target => rec.plan().owner.clone(),
            Locus::Host(HostRef::Static(h)) => h.clone(),
            Locus::Host(HostRef::Bound(b)) => return Err(b.clone()),
        };
        self.host_of(&name)
            .ok_or_else(|| format!("no host record for {name}"))
    }

    /// The executor for a host: `local()` for the controller; otherwise the
    /// first of the host's `reach` transports an executor serves.
    fn executor_for(&mut self, host: &Host) -> Option<&mut Box<dyn Executor>> {
        self.executor_index(host).map(|i| &mut self.executors[i])
    }

    /// The same choice as an index, for a caller that must also borrow the
    /// schedulers: two fields of the engine, borrowed disjointly.
    pub(crate) fn executor_index(&self, host: &Host) -> Option<usize> {
        let wanted: Vec<&str> = if host.name() == "controller" {
            vec!["local"]
        } else {
            host.record.reach.iter().map(String::as_str).collect()
        };
        for t in wanted {
            if let Some(i) = self
                .executors
                .iter()
                .position(|e| e.locus().transport() == t)
            {
                return Some(i);
            }
        }
        None
    }

    /// The probe a guard, observe or assert names, as the engine runs it:
    /// a declaration by that name or producing that fact, its body
    /// resolved, on its declared locus; with no declaration the name alone
    /// goes to the host's executor (a hook knows its probes by name; the
    /// real executors refuse an empty body).
    fn probe_run(
        &self,
        rec: &InstanceRecord,
        host: &Host,
        name: &str,
    ) -> Result<(Host, ProbeRun), String> {
        let decl = rec
            .plan()
            .probes
            .iter()
            .find(|p| p.name == name || p.produces.iter().any(|f| f == name))
            .cloned();
        match decl {
            Some(d) => {
                let on = match d.locus {
                    Locus::Controller => self.controller.clone(),
                    _ => host.clone(),
                };
                let env = self.env_for(rec, &BTreeMap::new());
                let body = resolve_body(&d.body, &on, &env).map_err(|u| u.to_string())?;
                Ok((
                    on,
                    ProbeRun {
                        name: d.name.clone(),
                        body,
                    },
                ))
            }
            None => Ok((
                host.clone(),
                ProbeRun {
                    name: name.to_string(),
                    body: Vec::new(),
                },
            )),
        }
    }

    fn observe_raw(
        &mut self,
        rec: &InstanceRecord,
        host: &Host,
        probe: &str,
    ) -> Result<Observation, String> {
        let (on, run) = self.probe_run(rec, host, probe)?;
        let ex = self
            .executor_for(&on)
            .ok_or_else(|| format!("no executor reaches {}", on.name()))?;
        ex.observe(&on, &run)
            .map_err(|e| format!("observing {probe} on {}: {e}", on.name()))
    }

    fn observe(&mut self, rec: &InstanceRecord, host: &Host, probe: &str) -> Result<Tri, String> {
        self.observe_raw(rec, host, probe).map(|o| o.as_tri())
    }

    fn observe_text(
        &mut self,
        rec: &InstanceRecord,
        host: &Host,
        probe: &str,
    ) -> Result<String, String> {
        self.observe_raw(rec, host, probe).map(|o| o.text)
    }

    // --- request -----------------------------------------------------------

    /// The instance id: plan, owner host and the parameters' hash (5.12).
    pub fn instance_id(ir: &PlanIr, params: &BTreeMap<String, String>) -> String {
        let ph = hash_json(&serde_json::to_value(params).unwrap_or_default())
            .map(|h| h.to_hex())
            .unwrap_or_else(|_| "00000000".into());
        format!("{}.{}.{}", ir.plan.id, ir.plan.owner, &ph[..8])
    }

    /// Check, request, approve (no gate) or hold pending (a gate), then
    /// apply. A rehearsal journals every step and reserves nothing.
    pub fn apply(
        &mut self,
        mut ir: PlanIr,
        params: BTreeMap<String, String>,
        opts: ApplyOptions,
    ) -> Result<Outcome, EngineError> {
        if let Some(m) = opts.mode {
            ir.plan.mode = m;
        }
        let verdict = check::check(&ir.site, &ir.requester, &ir.plan);
        if verdict.status != Status::Ok {
            return Err(EngineError::Refused(Box::new(verdict)));
        }
        let permanent = matches!(infer_intent(&ir.plan), Some(Intent::Permanent));
        let id = Engine::instance_id(&ir, &params);
        let now = self.clock.now();
        let mut rec = InstanceRecord {
            id: id.clone(),
            ir,
            params,
            state: State::Unchecked,
            permanent,
            rehearsal: opts.rehearsal,
            requested_at: None,
            approved_at: None,
            deadline: None,
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
            markers: BTreeMap::new(),
            dirs: Vec::new(),
            staged: Vec::new(),
            force_drift: false,
            backstop: None,
            nonce: hex(&nonce()),
            host_contract: String::new(),
            proofs: Vec::new(),
            secret_undelivered: false,
            attempting: None,
            acks: opts.acks.clone(),
            forced: opts.forced.clone(),
            ledger_ids: Vec::new(),
            refusal: None,
            closed_reason: None,
        };
        if let Some(existing) = self.store.read_instance::<InstanceRecord>(&id)? {
            if !states::terminal(existing.state) {
                return Err(EngineError::Ledger(
                    LedgerCode::R0101,
                    format!("instance {id} is already {}", existing.state),
                ));
            }
        }
        self.step(&mut rec, E::Check)?;
        self.log(&rec, J::Checked)?;
        // Request: reserve every touched host's umbra, unless rehearsing.
        let reservations = umbras(rec.plan());
        let mut ledger = self.ledger.clone();
        for (host, umbra) in reservations {
            let lid = format!("{id}@{host}");
            ledger = ledger
                .request(Held {
                    id: lid.clone(),
                    host,
                    umbra,
                    exclusivity: rec.plan().exclusivity.clone(),
                    rehearsal: rec.rehearsal,
                })
                .map_err(|(c, m)| EngineError::Ledger(c, m))?;
            if !rec.rehearsal {
                rec.ledger_ids.push(lid);
            }
        }
        self.ledger = ledger;
        self.store.write_ledger(&self.ledger)?;
        rec.requested_at = Some(now);
        // The host contract is frozen here: the records of every host the
        // plan touches and every `static: true` probe on them. The request
        // digest covers it, so a change invalidates the proofs (R0301).
        if !rec.rehearsal {
            rec.host_contract = hex(&self.host_contract(&rec).hash().0);
        }
        self.step(&mut rec, E::Request)?;
        self.log(&rec, J::Requested)?;
        if let Some(g) = rec.plan().gate.as_ref().map(|g| g.expr.clone()) {
            if !rec.rehearsal && !self.gate_satisfied(&rec, Scope::Plan, &g) {
                let window = rec
                    .plan()
                    .gate
                    .as_ref()
                    .and_then(|g| g.window)
                    .or(rec.ir.site.max_wait);
                rec.approval_deadline = window.map(|w| now.plus(w));
                self.persist(&rec)?;
                return Ok(self.outcome(&rec));
            }
        }
        self.approve(&mut rec)?;
        self.persist(&rec)?;
        self.walk(&mut rec)?;
        Ok(self.outcome(&rec))
    }

    pub(crate) fn approve(&mut self, rec: &mut InstanceRecord) -> Result<(), EngineError> {
        if self.refuse_on_contract_change(rec)? {
            return Ok(());
        }
        let now = self.clock.now();
        rec.approved_at = Some(now);
        if !rec.permanent {
            rec.deadline = effective_wane(rec.plan()).map(|w| now.plus(w));
        }
        self.step(rec, E::Approve)?;
        self.log(
            rec,
            J::Approved {
                rehearsal: rec.rehearsal,
            },
        )?;
        Ok(())
    }

    /// Feed one event to the machine; refuse per its verdict; persist.
    pub(crate) fn step(
        &mut self,
        rec: &mut InstanceRecord,
        ev: E,
    ) -> Result<Outcome_, EngineError> {
        let outcome = states::transition(rec.ctx(), rec.state, ev);
        if outcome != Verdict_::NotApplicable {
            self.trace.push(Transition {
                id: rec.id.clone(),
                from: rec.state,
                event: ev,
                outcome,
            });
        }
        match outcome {
            Verdict_::To(s) => {
                rec.state = s;
                self.persist(rec)?;
                Ok(Outcome_::Moved)
            }
            Verdict_::Stay => Ok(Outcome_::Stayed),
            Verdict_::Refuse(code) => Err(EngineError::NotAdmitted(
                code,
                format!("{ev} in {} ({:?} plan)", rec.state, rec.intent()),
            )),
            Verdict_::NotApplicable => Err(EngineError::WrongState {
                state: rec.state,
                verb: ev.to_string(),
            }),
        }
    }

    // --- the walk ----------------------------------------------------------

    /// Walk the plan from its start, skipping what is applied, until it
    /// ends or the instance leaves `Applying`.
    pub(crate) fn walk(&mut self, rec: &mut InstanceRecord) -> Result<(), EngineError> {
        if rec.state != State::Applying {
            return Ok(());
        }
        // Re-derived at apply, the third of the three times 5.11 names.
        if self.refuse_on_contract_change(rec)? {
            return Ok(());
        }
        let items = rec.plan().body.clone();
        let mut leaf = 1u32;
        let mut flow = self.walk_items(rec, &items, 0, &mut leaf, &BTreeMap::new())?;
        if matches!(flow, Flow::Continue) && rec.state == State::Applying {
            flow = self.backstop_after_walk(rec)?;
        }
        if matches!(flow, Flow::Continue) && rec.state == State::Applying {
            // Every leaf done and no commit() item: a temporary plan rests.
            self.step(rec, E::AllStepsDone)?;
            self.log(rec, J::Applied)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_items(
        &mut self,
        rec: &mut InstanceRecord,
        items: &[Item],
        iteration: u32,
        leaf: &mut u32,
        vars: &BTreeMap<String, String>,
    ) -> Result<Flow, EngineError> {
        for it in items {
            let flow = match it {
                Item::Par { children } => self.walk_items(rec, children, iteration, leaf, vars)?,
                Item::Repeat { form, var, body } => {
                    let start = *leaf;
                    let count = leaf_count(body);
                    let values: Vec<String> = match form {
                        RepeatForm::Count(n) => (0..*n).map(|i| i.to_string()).collect(),
                        RepeatForm::Over { list, .. } => self.list_value(rec, list, vars)?,
                    };
                    let mut flow = Flow::Continue;
                    for (i, v) in values.iter().enumerate() {
                        let mut inner = vars.clone();
                        inner.insert(var.clone(), v.clone());
                        let mut l = start;
                        flow = self.walk_items(rec, body, i as u32, &mut l, &inner)?;
                        if matches!(flow, Flow::Stop) {
                            break;
                        }
                    }
                    *leaf = start + count;
                    flow
                }
                Item::When {
                    guard,
                    then_,
                    else_,
                    ..
                } => {
                    let key = format!("w{}", *leaf);
                    let choice = match rec.choices.get(&key) {
                        Some(c) => *c,
                        None => {
                            let owner = self.owner_host(rec)?;
                            let tri = self.observe(rec, &owner, &guard.name);
                            let c = match tri {
                                Ok(Tri::Yes) => true,
                                Ok(Tri::No) => false,
                                Ok(Tri::Unknown) => guard.value == Tri::Yes,
                                Err(e) => {
                                    return self.refuse(
                                        rec,
                                        *leaf,
                                        &format!("when guard {}: {e}", guard.name),
                                    );
                                }
                            };
                            rec.choices.insert(key, c);
                            self.persist(rec)?;
                            c
                        }
                    };
                    let then_count = leaf_count(then_);
                    let else_count = leaf_count(else_);
                    if choice {
                        let f = self.walk_items(rec, then_, iteration, leaf, vars)?;
                        *leaf += else_count;
                        f
                    } else {
                        *leaf += then_count;
                        self.walk_items(rec, else_, iteration, leaf, vars)?
                    }
                }
                leaf_item => {
                    let n = *leaf;
                    *leaf += 1;
                    if rec.is_applied(n, iteration) {
                        continue;
                    }
                    self.run_leaf(rec, n, iteration, leaf_item, vars)?
                }
            };
            if matches!(flow, Flow::Stop) {
                return Ok(Flow::Stop);
            }
        }
        Ok(Flow::Continue)
    }

    fn owner_host(&self, rec: &InstanceRecord) -> Result<Host, EngineError> {
        self.host_of(&rec.plan().owner).ok_or_else(|| {
            EngineError::Internal(format!("no host record for owner {}", rec.plan().owner))
        })
    }

    fn list_value(
        &mut self,
        rec: &InstanceRecord,
        list: &str,
        vars: &BTreeMap<String, String>,
    ) -> Result<Vec<String>, EngineError> {
        let text = if let Some(v) = vars.get(list) {
            v.clone()
        } else if let Some(v) = rec.params.get(list) {
            v.clone()
        } else if let Some(v) = rec.outputs.get(list) {
            v.clone()
        } else {
            let owner = self.owner_host(rec)?;
            self.observe_text(rec, &owner, list)
                .map_err(|e| EngineError::Internal(format!("repeat over {list}: {e}")))?
        };
        Ok(text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn env_for(&self, rec: &InstanceRecord, vars: &BTreeMap<String, String>) -> Env {
        Env {
            params: rec.params.clone(),
            outputs: rec.outputs.clone(),
            controller: vars.clone(),
            facts: BTreeMap::new(),
            secrets: BTreeMap::new(),
        }
    }

    /// Install the backstop before the first covered step and arm it
    /// before the step `arm_before` names (5.6). A refusal here refuses
    /// the step: no step is committed before the backstop covering it is
    /// armed.
    fn backstop_before_leaf(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
    ) -> Result<Option<Flow>, EngineError> {
        let Some(cov) = rue_core::backstop::coverage(rec.plan()) else {
            return Ok(None);
        };
        if cov.covered.is_empty() {
            return Ok(None);
        }
        if cov.installed_before.is_some_and(|first| n >= first) {
            if let Err(why) = self.backstop_install(rec)? {
                return self.refuse(rec, n, &why).map(Some);
            }
        }
        if n >= cov.armed_before_step {
            if let Err(why) = self.backstop_arm(rec)? {
                return self.refuse(rec, n, &why).map(Some);
            }
        }
        Ok(None)
    }

    /// Late arming: `arm_before` past the last step, so the backstop is
    /// armed once the covered steps are done and the verdict states the
    /// engine-only window.
    fn backstop_after_walk(&mut self, rec: &mut InstanceRecord) -> Result<Flow, EngineError> {
        let Some(cov) = rue_core::backstop::coverage(rec.plan()) else {
            return Ok(Flow::Continue);
        };
        if cov.covered.is_empty() || rec.backstop.as_ref().is_some_and(|b| b.armed) {
            return Ok(Flow::Continue);
        }
        if let Err(why) = self.backstop_install(rec)? {
            return self.refuse(rec, cov.armed_before_step, &why);
        }
        if let Err(why) = self.backstop_arm(rec)? {
            return self.refuse(rec, cov.armed_before_step, &why);
        }
        Ok(Flow::Continue)
    }

    fn run_leaf(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        iteration: u32,
        it: &Item,
        vars: &BTreeMap<String, String>,
    ) -> Result<Flow, EngineError> {
        if let Some(flow) = self.backstop_before_leaf(rec, n)? {
            return Ok(flow);
        }
        match it {
            Item::Step(s) => self.run_step(rec, n, iteration, s, false, vars),
            Item::Knell(s) => self.run_step(rec, n, iteration, s, true, vars),
            Item::Confirm => {
                self.step(rec, E::Confirm)?;
                if let Err(why) = self.backstop_disarm(rec, Disarm::Confirmed)? {
                    return self.refuse(rec, n, &why);
                }
                self.log(rec, J::Confirmed)?;
                self.mark_applied(rec, n, iteration)?;
                Ok(Flow::Continue)
            }
            Item::Commit => {
                self.step(rec, E::CommitItem)?;
                self.log(
                    rec,
                    J::Committed {
                        by: rec.ir.requester.clone(),
                        reason: "commit() item".into(),
                    },
                )?;
                self.release(rec)?;
                self.remove_dirs(rec)?;
                self.persist(rec)?;
                Ok(Flow::Stop)
            }
            Item::Observe { probe, alias } => {
                let owner = self.owner_host(rec)?;
                match self.observe_text(rec, &owner, probe) {
                    Ok(text) => {
                        rec.outputs.insert(alias.clone(), text);
                        self.mark_applied(rec, n, iteration)?;
                        Ok(Flow::Continue)
                    }
                    Err(e) => self.refuse(rec, n, &e),
                }
            }
            Item::Preflight { guards } => {
                for g in guards {
                    if let Some(flow) = self.guard_blocks(rec, n, g, &[])? {
                        return Ok(flow);
                    }
                }
                self.mark_applied(rec, n, iteration)?;
                Ok(Flow::Continue)
            }
            Item::Assert { guard, window, .. } => {
                let bound = window.or(rec.ir.site.max_wait);
                if let Some(flow) = self.guard_blocks_with_bound(rec, n, guard, &[], bound)? {
                    return Ok(flow);
                }
                self.mark_applied(rec, n, iteration)?;
                Ok(Flow::Continue)
            }
            Item::Slot { name } => Err(EngineError::Internal(format!(
                "slot {name} reached the engine unfilled"
            ))),
            Item::Par { .. } | Item::Repeat { .. } | Item::When { .. } => {
                Err(EngineError::Internal("a container reached run_leaf".into()))
            }
        }
    }

    fn mark_applied(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        iteration: u32,
    ) -> Result<(), EngineError> {
        rec.applied.push(AppliedStep { step: n, iteration });
        rec.attempting = None;
        self.persist(rec)
    }

    /// Evaluate a guard on the owner host. `None` means it passed; `Some`
    /// carries the flow after a wait or a refusal.
    fn guard_blocks(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        g: &Guard,
        force: &[ForceName],
    ) -> Result<Option<Flow>, EngineError> {
        let bound = rec.ir.site.max_wait;
        self.guard_blocks_with_bound(rec, n, g, force, bound)
    }

    fn guard_blocks_with_bound(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        g: &Guard,
        force: &[ForceName],
        bound: Option<Duration>,
    ) -> Result<Option<Flow>, EngineError> {
        let owner = self.owner_host(rec)?;
        let tri = match self.observe(rec, &owner, &g.name) {
            Ok(t) => t,
            Err(e) => return self.refuse(rec, n, &e).map(Some),
        };
        match tri {
            Tri::Yes => Ok(None),
            Tri::No => self
                .refuse(rec, n, &format!("guard {} is no", g.name))
                .map(Some),
            Tri::Unknown => {
                let forced = !g.force_never
                    && rec.plan().mode == Mode::Manual
                    && (rec.forced.iter().any(|f| f == &g.name)
                        || force.iter().any(|f| {
                            matches!(f, ForceName::Guard(x) if x == &g.name)
                                || matches!(f, ForceName::Unknown)
                        }));
                if forced {
                    Ok(None)
                } else {
                    self.wait(
                        rec,
                        n,
                        &format!("unknown guard {}", g.name),
                        Some(g.name.clone()),
                        bound,
                    )
                    .map(Some)
                }
            }
        }
    }

    fn wait(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        reason: &str,
        guard: Option<String>,
        bound: Option<Duration>,
    ) -> Result<Flow, EngineError> {
        let now = self.clock.now();
        rec.waiting = Some(Wait {
            step: n,
            reason: reason.to_string(),
            since: now,
            bound: bound.map(|b| now.plus(b)),
            guard,
        });
        self.step(rec, E::WaitAtStep)?;
        self.log(
            rec,
            J::Waiting {
                step: n,
                reason: reason.to_string(),
            },
        )?;
        Ok(Flow::Stop)
    }

    fn run_step(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        iteration: u32,
        s: &StepI,
        knell: bool,
        vars: &BTreeMap<String, String>,
    ) -> Result<Flow, EngineError> {
        let op = s.op.clone();
        // A bound host, or one no transport reaches, defers the step.
        let host = match self.step_host(rec, &op) {
            Ok(h) => h,
            Err(from) => return self.defer(rec, n, &op, &from),
        };
        // A rehearsal calls no executor, so it is never deferred for the
        // lack of one (the check's own deferrals still stand).
        if !rec.rehearsal && host.name() != "controller" && self.executor_for(&host).is_none() {
            return self.defer(
                rec,
                n,
                &op,
                &format!("no transport reaches {}", host.name()),
            );
        }
        // R0408: a :target undo needs a filesystem on the host, decided
        // before do.
        let caps = self.caps_of(&host);
        if op.undo_locus == UndoLocus::Target && !caps.filesystem && !rec.rehearsal {
            return self.refuse(
                rec,
                n,
                &format!(
                    "R0408: a :target undo on {}, whose executor reports no filesystem",
                    host.name()
                ),
            );
        }
        // A step gate: satisfied by the proofs for this step's scope and
        // the wait accrued since the request, or the instance waits.
        if let Some(g) = s.gate.clone() {
            if !rec.rehearsal && !self.gate_satisfied(rec, Scope::Step(n), &g) {
                self.log(
                    rec,
                    J::StepGateRequested {
                        step: n,
                        step_digest: self.scope_digest(rec, Scope::Step(n)),
                    },
                )?;
                let bound = s.window.or(rec.ir.site.max_wait);
                return self.wait(rec, n, "gate", None, bound);
            }
        }
        for g in &op.pre {
            if let Some(flow) = self.guard_blocks(rec, n, g, &s.force)? {
                return Ok(flow);
            }
        }
        if knell {
            if let Refusal::Knell { guard, cost, ack } = &op.refusal {
                if let Some(g) = guard {
                    if let Some(flow) = self.guard_blocks(rec, n, g, &s.force)? {
                        return Ok(flow);
                    }
                }
                let cost_text = match cost {
                    rue_core::model::Cost::Probe(p) => p.clone(),
                    rue_core::model::Cost::NoCost(_) => "none".into(),
                };
                if matches!(ack, Ack::Gate(_)) {
                    if rec.acks.contains(&n) {
                        self.log(
                            rec,
                            J::KnellAcknowledged {
                                step: n,
                                cost: cost_text,
                                by: rec.ir.requester.clone(),
                            },
                        )?;
                    } else {
                        self.log(
                            rec,
                            J::AckRequested {
                                step: n,
                                cost: cost_text,
                            },
                        )?;
                        let bound = s.window.or(rec.ir.site.max_wait);
                        return self.wait(rec, n, "ack", None, bound);
                    }
                }
            }
        }
        // Write-ahead: the entry is acknowledged and the record persisted in
        // Applying before anything runs.
        self.log(
            rec,
            J::Applying {
                step: n,
                undo_line: undo_line(&op),
            },
        )?;
        // From here until the step ends, the record says which step is
        // in flight: an engine that dies now must undo it on the way back
        // even though it was never marked applied.
        rec.attempting = Some(AppliedStep { step: n, iteration });
        self.persist(rec)?;
        if rec.rehearsal {
            self.log(rec, J::StepDone { step: n })?;
            return self.after_step(rec, n, iteration, &op, vars);
        }
        if let Err(why) = self.ensure_instance_dir(rec, &host)? {
            return self.refuse(rec, n, &why);
        }
        self.snapshot(rec, n, &op, &host)?;
        let before = self.watched(rec, &host, n);
        let env = self.env_for(rec, vars);
        let body = match resolve_body(&op.do_, &host, &env) {
            Ok(b) => b,
            Err(u) => return self.refuse(rec, n, &format!("step {n}: {u}")),
        };
        let id = rec.id.clone();
        let result = {
            let ex = self
                .executor_for(&host)
                .ok_or_else(|| EngineError::Internal("executor vanished".into()))?;
            ex.run(&host, &id, &body)
        };
        match result {
            Ok(out) => {
                // Every declared output must be present: silence is refusal.
                let missing: Vec<&str> = op
                    .outputs
                    .iter()
                    .filter(|o| !out.outputs.contains_key(&o.name))
                    .map(|o| o.name.as_str())
                    .collect();
                if !missing.is_empty() {
                    let why = format!("silent: no output for {}", missing.join(", "));
                    self.log(
                        rec,
                        J::StepFailed {
                            step: n,
                            error: why.clone(),
                        },
                    )?;
                    rec.refusal = Some(format!("step {n}: {why}"));
                    self.undo_failed(rec, n, iteration, &op)?;
                    return self.refuse_applied(rec, n);
                }
                // R0201: nothing outside the step's footprint changed.
                let after = self.watched(rec, &host, n);
                let changed = footprint::changed_paths(&before, &after);
                if !changed.is_empty() {
                    self.log(
                        rec,
                        J::FootprintViolation {
                            step: n,
                            facts: changed.iter().map(|p| format!("file:{p}")).collect(),
                        },
                    )?;
                    rec.refusal = Some(format!(
                        "step {n}: R0201: footprint violation: {}",
                        changed.join(", ")
                    ));
                    self.undo_failed(rec, n, iteration, &op)?;
                    return self.refuse_applied(rec, n);
                }
                self.write_markers(rec, &host, n, &op)?;
                for p in &op.do_ {
                    if let rue_core::body::Prim::Stage(st) = p {
                        rec.staged.push(Staged {
                            host: host.name().to_string(),
                            step: n,
                            name: st.name.clone(),
                        });
                    }
                }
                let alias = s.alias.clone().unwrap_or_else(|| op.id.clone());
                let mut secrets: Vec<(String, String)> = Vec::new();
                for o in &op.outputs {
                    let Some(v) = out.outputs.get(&o.name) else {
                        continue;
                    };
                    if o.secret {
                        secrets.push((format!("{alias}.{}", o.name), v.clone()));
                    } else {
                        rec.outputs.insert(format!("{alias}.{}", o.name), v.clone());
                    }
                }
                self.log(rec, J::StepDone { step: n })?;
                // Delivery is at the completion the journal just recorded:
                // the credential is live from that instant, and the step's
                // undo is what revokes it (5.13).
                for (label, value) in &secrets {
                    self.deliver_secret(rec, label, value)?;
                }
                self.after_step(rec, n, iteration, &op, vars)
            }
            Err(e) => {
                self.log(
                    rec,
                    J::StepFailed {
                        step: n,
                        error: e.to_string(),
                    },
                )?;
                // The step failed: the instance closes because something
                // refused it, which is what exit 1 says (6.8).
                rec.refusal = Some(format!("step {n}: {e}"));
                self.undo_failed(rec, n, iteration, &op)?;
                self.refuse_applied(rec, n)
            }
        }
    }

    /// A failed step is itself reverted at once (it may be half-applied).
    /// If that undo fails too, the step stays in the applied set so the
    /// revert retries it and reports it stuck.
    fn undo_failed(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        iteration: u32,
        op: &Op,
    ) -> Result<(), EngineError> {
        rec.attempting = None;
        if let Err(why) = self.undo_step(rec, n, op) {
            rec.applied.push(AppliedStep { step: n, iteration });
            self.log(
                rec,
                J::StepFailed {
                    step: n,
                    error: format!("undo: {why}"),
                },
            )?;
        }
        self.persist(rec)
    }

    /// After a step's `do`: mark it applied, then its post guards.
    fn after_step(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        iteration: u32,
        op: &Op,
        _vars: &BTreeMap<String, String>,
    ) -> Result<Flow, EngineError> {
        self.mark_applied(rec, n, iteration)?;
        if rec.rehearsal {
            return Ok(Flow::Continue);
        }
        for g in &op.post {
            if let Some(flow) = self.guard_blocks(rec, n, g, &[])? {
                return Ok(flow);
            }
        }
        self.remove_staged(rec, Some(n), "step done")?;
        Ok(Flow::Continue)
    }

    /// The capabilities of the executor that reaches a host; none when
    /// nothing does.
    fn caps_of(&mut self, host: &Host) -> ExecCaps {
        self.executor_for(host)
            .map(|e| e.capabilities())
            .unwrap_or(ExecCaps {
                filesystem: false,
                stdin_preamble: false,
            })
    }

    /// An instance directory on a run-capable host, created before the
    /// first step there (7.7). A host that is not bootstrapped refuses
    /// (R0407).
    pub(crate) fn ensure_instance_dir(
        &mut self,
        rec: &mut InstanceRecord,
        host: &Host,
    ) -> Result<Result<(), String>, EngineError> {
        if rec.rehearsal
            || !self.caps_of(host).filesystem
            || rec.dirs.contains(&host.name().to_string())
        {
            return Ok(Ok(()));
        }
        let id = rec.id.clone();
        let ex = match self.executor_for(host) {
            Some(ex) => ex,
            None => return Ok(Ok(())),
        };
        match ex.bootstrap_state(host) {
            Ok(st) if st.ready() => {}
            Ok(st) => {
                return Ok(Err(format!(
                    "R0407: {} is not bootstrapped (rue_root {}, group {}, instances {}, lock {}, modes {}); run `rue bootstrap {}`",
                    host.name(),
                    st.rue_root,
                    st.group,
                    st.instances_dir,
                    st.lock,
                    st.modes_ok,
                    host.name()
                )))
            }
            Err(e) => return Ok(Err(format!("R0407: {}: {e}", host.name()))),
        }
        if let Err(e) = ex.instance_dir_create(host, &id) {
            return Ok(Err(format!("instance directory on {}: {e}", host.name())));
        }
        rec.dirs.push(host.name().to_string());
        self.persist(rec)?;
        Ok(Ok(()))
    }

    /// Digests of every file fact of the plan on a host, except step `n`'s
    /// own: what `do` must leave alone (R0201).
    fn watched(&mut self, rec: &InstanceRecord, host: &Host, n: u32) -> Watched {
        let mut w = Watched::new();
        if rec.rehearsal {
            return w;
        }
        let mut paths: Vec<String> = Vec::new();
        for (m, it) in numbered(&rec.plan().body) {
            if m == n {
                continue;
            }
            if let Some(o) = op_of(it) {
                if self.step_host(rec, o).map(|h| h.name() == host.name()) != Ok(true) {
                    continue;
                }
                for (_, _, p) in footprint::file_facts(&o.footprint) {
                    paths.push(p.to_string());
                }
            }
        }
        let own: Vec<String> = rec
            .op_at(n)
            .map(|o| {
                footprint::file_facts(&o.footprint)
                    .into_iter()
                    .map(|(_, _, p)| p.to_string())
                    .collect()
            })
            .unwrap_or_default();
        if let Some(ex) = self.executor_for(host) {
            for p in paths {
                if own.contains(&p) || w.contains_key(&p) {
                    continue;
                }
                let d = footprint::digest_of(
                    ex.read_fact(host, &format!("file:{p}"))
                        .ok()
                        .flatten()
                        .as_deref(),
                );
                w.insert(p, d);
            }
        }
        w
    }

    /// The markers of step `n`'s file facts as `do` left them, and the
    /// manifest of regions held on the host, to the record and to the
    /// instance directory.
    fn write_markers(
        &mut self,
        rec: &mut InstanceRecord,
        host: &Host,
        n: u32,
        op: &Op,
    ) -> Result<(), EngineError> {
        let id = rec.id.clone();
        let has_dir = rec.dirs.contains(&host.name().to_string());
        let mut markers = Vec::new();
        if let Some(ex) = self.executor_for(host) {
            for (_, e, p) in footprint::file_facts(&op.footprint) {
                let d =
                    footprint::digest_of(ex.read_fact(host, &e.shape).ok().flatten().as_deref());
                markers.push(Marker {
                    kind: e.kind,
                    path: p.to_string(),
                    digest: d,
                });
            }
            if has_dir {
                ex.put_file(
                    host,
                    &id,
                    &format!("markers/{n}"),
                    footprint::markers_text(&markers).as_bytes(),
                    0o640,
                )
                .map_err(|e| EngineError::Internal(format!("markers on {}: {e}", host.name())))?;
            }
        }
        rec.markers.insert(n.to_string(), markers);
        if has_dir {
            let regions = self.regions_on(rec, host);
            if let Some(ex) = self.executor_for(host) {
                ex.replace_file(
                    host,
                    &id,
                    "manifest",
                    footprint::manifest_text(&regions).as_bytes(),
                )
                .map_err(|e| EngineError::Internal(format!("manifest on {}: {e}", host.name())))?;
            }
        }
        self.persist(rec)
    }

    /// Every region this instance holds on a host: the applied steps'
    /// region facts with their anchors, plus the step being marked.
    fn regions_on(&self, rec: &InstanceRecord, host: &Host) -> Vec<(String, String)> {
        let mut v = Vec::new();
        let mut steps: Vec<u32> = rec.applied.iter().map(|a| a.step).collect();
        steps.extend(rec.markers.keys().filter_map(|k| k.parse::<u32>().ok()));
        steps.sort();
        steps.dedup();
        for n in steps {
            if let Some(o) = rec.op_at(n) {
                if self.step_host(rec, o).map(|h| h.name() == host.name()) != Ok(true) {
                    continue;
                }
                for e in &o.footprint {
                    if e.kind == Kind::Region {
                        if let (Some(p), Some(anchor)) = (region::file_path(&e.shape), &e.anchor) {
                            v.push((p.to_string(), anchor.clone()));
                        }
                    }
                }
            }
        }
        v.sort();
        v.dedup();
        v
    }

    /// Whether another active instance holds a region on a file of this
    /// host (the foreign-region condition, from the ledger).
    fn foreign_region(&self, rec: &InstanceRecord, host: &Host, shape: &str) -> bool {
        self.ledger.holdings().iter().any(|h| {
            !rec.ledger_ids.contains(&h.id)
                && h.host == host.name()
                && h.umbra
                    .iter()
                    .any(|f| f.shape == shape && f.anchor.is_some())
        })
    }

    /// Remove staged files: of one step after it ran, or of every step at
    /// boot for an instance that is not applying.
    fn remove_staged(
        &mut self,
        rec: &mut InstanceRecord,
        step: Option<u32>,
        reason: &str,
    ) -> Result<(), EngineError> {
        let mine: Vec<Staged> = rec
            .staged
            .iter()
            .filter(|s| step.is_none_or(|n| s.step == n))
            .cloned()
            .collect();
        if mine.is_empty() {
            return Ok(());
        }
        let id = rec.id.clone();
        for s in &mine {
            if let Some(host) = self.host_of(&s.host) {
                if let Some(ex) = self.executor_for(&host) {
                    let _ = ex.remove_file(&host, &id, &s.name);
                }
            }
            self.log(
                rec,
                J::StagedRemoved {
                    step: s.step,
                    reason: reason.to_string(),
                },
            )?;
        }
        rec.staged.retain(|s| !mine.contains(s));
        self.persist(rec)
    }

    /// Remove the instance directories at close or commit.
    fn remove_dirs(&mut self, rec: &mut InstanceRecord) -> Result<(), EngineError> {
        if let Err(why) = self.backstop_disarm(rec, Disarm::All)? {
            self.log(rec, J::Refused { reason: why })?;
        }
        let id = rec.id.clone();
        for name in rec.dirs.clone() {
            if let Some(host) = self.host_of(&name) {
                if let Some(ex) = self.executor_for(&host) {
                    let _ = ex.instance_dir_remove(&host, &id);
                }
            }
        }
        rec.dirs.clear();
        self.persist(rec)
    }

    fn snapshot(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        op: &Op,
        host: &Host,
    ) -> Result<(), EngineError> {
        for (k, e) in op.footprint.iter().enumerate() {
            if !matches!(e.kind, Kind::Modified | Kind::Region) || !e.shape.starts_with("file:") {
                continue;
            }
            let ex = match self.executor_for(host) {
                Some(ex) => ex,
                None => continue,
            };
            if let Ok(Some(bytes)) = ex.read_fact(host, &e.shape) {
                // R0204: the cap the verdict states is the cap the engine
                // keeps. A fact above it is not snapshotted, and the step
                // that wanted the snapshot is refused rather than left
                // with an undo it cannot perform.
                if bytes.len() as u64 > SNAPSHOT_CAP {
                    return Err(EngineError::Runtime(format!(
                        "R0204: {} on {} is {} bytes, above the {SNAPSHOT_CAP}-byte snapshot cap",
                        e.shape,
                        host.name(),
                        bytes.len()
                    )));
                }
                if rec.dirs.contains(&host.name().to_string()) {
                    let _ = ex.replace_file(host, &rec.id, &format!("snapshots/{n}/{k}"), &bytes);
                }
                rec.snapshots.insert(
                    format!("{n}/{k}"),
                    String::from_utf8_lossy(&bytes).into_owned(),
                );
            }
        }
        self.persist(rec)
    }

    fn defer(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        op: &Op,
        why: &str,
    ) -> Result<Flow, EngineError> {
        let handoff = op
            .handoff_done
            .clone()
            .unwrap_or_else(|| format!("handoff-done {n}"));
        rec.deferred = Some(DeferredAt {
            step: n,
            handoff: handoff.clone(),
        });
        self.step(rec, E::DeferAtStep)?;
        self.log(
            rec,
            J::Deferred {
                step: n,
                handoff: format!("{handoff} ({why})"),
            },
        )?;
        Ok(Flow::Stop)
    }

    /// A refusal at step `n` before its `do` ran: rule 4.
    fn refuse(
        &mut self,
        rec: &mut InstanceRecord,
        n: u32,
        reason: &str,
    ) -> Result<Flow, EngineError> {
        rec.refusal = Some(format!("step {n}: {reason}"));
        self.log(
            rec,
            J::Refused {
                reason: rec.refusal.clone().unwrap_or_default(),
            },
        )?;
        self.refuse_applied(rec, n)
    }

    /// The refusal proper: Held when an earlier applied step holds, else
    /// Reverting, and the revert runs.
    fn refuse_applied(&mut self, rec: &mut InstanceRecord, n: u32) -> Result<Flow, EngineError> {
        self.step(rec, E::Refuse)?;
        if rec.state == State::Held {
            rec.held_at = Some(n);
            self.persist(rec)?;
            self.log(rec, J::Held { step: n })?;
            return Ok(Flow::Stop);
        }
        self.revert(rec)?;
        Ok(Flow::Stop)
    }

    // --- revert ------------------------------------------------------------

    /// Undo the applied steps last-in-first-out. Clean: Closed. A failing
    /// undo: Stuck, retried each reap pass from where it stopped.
    fn revert(&mut self, rec: &mut InstanceRecord) -> Result<(), EngineError> {
        let steps: Vec<u32> = {
            let mut v: Vec<u32> = rec.applied.iter().rev().map(|a| a.step).collect();
            v.dedup();
            v
        };
        self.log(rec, J::Reverting { steps })?;
        rec.stuck.clear();
        while let Some(a) = rec.applied.last().cloned() {
            let op = match rec.op_at(a.step) {
                Some(o) => o.clone(),
                None => {
                    // confirm(), observe, assert, preflight: nothing to undo.
                    rec.applied.pop();
                    self.persist(rec)?;
                    continue;
                }
            };
            match self.undo_step(rec, a.step, &op) {
                Ok(report) if !report.held.is_empty() => {
                    rec.drift_held = vec![a.step];
                    self.log(
                        rec,
                        J::DriftHeld {
                            step: a.step,
                            facts: report.held,
                        },
                    )?;
                    self.step(rec, E::DriftOnDefer)?;
                    return Ok(());
                }
                Ok(report) => {
                    if !report.clobbered.is_empty() {
                        // R0202 is informational: the drift was seen and
                        // the step's policy applied to it.
                        self.log(
                            rec,
                            J::DriftClobbered {
                                step: a.step,
                                facts: report.clobbered,
                            },
                        )?;
                    }
                    rec.markers.remove(&a.step.to_string());
                    rec.applied.pop();
                    self.persist(rec)?;
                }
                Err(why) => {
                    rec.stuck = rec.applied.iter().rev().map(|x| x.step).collect();
                    rec.stuck.dedup();
                    self.log(
                        rec,
                        J::StepFailed {
                            step: a.step,
                            error: format!("undo: {why}"),
                        },
                    )?;
                    self.step(rec, E::UndoFailed)?;
                    self.log(
                        rec,
                        J::Stuck {
                            steps: rec.stuck.clone(),
                        },
                    )?;
                    return Ok(());
                }
            }
        }
        self.step(rec, E::UndoClean)?;
        self.log(rec, J::Reverted)?;
        let reason = rec
            .refusal
            .clone()
            .map(|r| format!("reverted after refusal: {r}"))
            .unwrap_or_else(|| "reverted".into());
        rec.closed_reason = Some(reason.clone());
        self.log(
            rec,
            J::Closed {
                reason: reason.clone(),
            },
        )?;
        self.drop_secrets(rec, &reason)?;
        self.release(rec)?;
        self.remove_dirs(rec)?;
        self.persist(rec)
    }

    /// Undo one step under its drift policy (5.2). Every file fact is read
    /// against its marker; the decision per fact is the artifact's
    /// (`footprint::decide`). Any fact deferred leaves the step untouched
    /// and reported `held`; otherwise the undo runs, under the host lock
    /// when the step holds a region, with a damaged region restored whole
    /// from its snapshot where the policy and the ledger allow it.
    fn undo_step(&mut self, rec: &InstanceRecord, n: u32, op: &Op) -> Result<UndoReport, String> {
        let mut report = UndoReport::default();
        if rec.rehearsal || matches!(op.undo, Undo::NoUndo) {
            return Ok(report);
        }
        let host = self.step_host(rec, op)?;
        let env = self.env_for(rec, &BTreeMap::new());
        let policy = op.effective_drift().unwrap_or(Drift::Clobber);
        let markers = rec.markers.get(&n.to_string()).cloned().unwrap_or_default();
        let mut whole: Vec<usize> = Vec::new();
        let has_region = op.footprint.iter().any(|e| e.kind == Kind::Region);
        let id = rec.id.clone();
        let facts: Vec<(usize, FootprintEntry, String)> = footprint::file_facts(&op.footprint)
            .into_iter()
            .map(|(k, e, p)| (k, e.clone(), p.to_string()))
            .collect();
        // Whether a sibling instance holds a region on each fact: the
        // engine's own ledger, read before the host lock because nothing
        // on the host can change it.
        let foreign_by_shape: BTreeMap<String, bool> = facts
            .iter()
            .filter(|(_, e, _)| e.kind == Kind::Region)
            .map(|(_, e, _)| (e.shape.clone(), self.foreign_region(rec, &host, &e.shape)))
            .collect();
        let idx = self
            .executor_index(&host)
            .ok_or_else(|| format!("no executor reaches {}", host.name()))?;
        // The host lock across a region undo: every read the decision
        // makes and every write that follows see one world (5.2, 7.7).
        let _lock = if has_region {
            Some(
                self.executors[idx]
                    .host_lock(&host)
                    .map_err(|e| format!("host lock on {}: {e}", host.name()))?,
            )
        } else {
            None
        };
        for (k, e, p) in &facts {
            let bytes = self.executors[idx]
                .read_fact(&host, &e.shape)
                .ok()
                .flatten();
            let current = footprint::digest_of(bytes.as_deref());
            let recorded = markers
                .iter()
                .find(|m| m.path == *p)
                .map(|m| m.digest.clone());
            let changed = recorded.as_ref().is_some_and(|r| *r != current);
            let intact = if e.kind == Kind::Region {
                let text = bytes
                    .as_deref()
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default();
                Some(region::intact(&text, e.anchor.as_deref().unwrap_or("")))
            } else {
                None
            };
            let foreign = foreign_by_shape.get(&e.shape).copied().unwrap_or(false);
            let mut d = footprint::decide(e.kind, policy, changed, intact, foreign);
            if rec.force_drift {
                d = match d {
                    Decision::Defer => Decision::Clobber,
                    Decision::DeferForeign => Decision::RestoreWhole,
                    other => other,
                };
            }
            match d {
                Decision::Undo => {}
                Decision::Clobber => report.clobbered.push(e.shape.clone()),
                Decision::RestoreWhole => {
                    report.clobbered.push(e.shape.clone());
                    whole.push(*k);
                }
                Decision::Defer | Decision::DeferForeign => report.held.push(e.shape.clone()),
            }
        }
        if !report.held.is_empty() {
            return Ok(report);
        }
        let mut body: Vec<RPrim> = match &op.undo {
            Undo::NoUndo => Vec::new(),
            Undo::Restore => restore_body(n, &op.footprint, &rec.snapshots)?,
            Undo::Computed { body, .. } | Undo::Compensate { body, .. } => {
                resolve_body(body, &host, &env).map_err(|u| u.to_string())?
            }
        };
        // A damaged region restored whole: its strip becomes the snapshot
        // written back.
        for k in whole {
            let e = &op.footprint[k];
            let snapshot = rec
                .snapshots
                .get(&format!("{n}/{k}"))
                .cloned()
                .ok_or_else(|| format!("no snapshot for {} to restore whole", e.shape))?;
            for prim in body.iter_mut() {
                if matches!(prim, RPrim::RegionClear { shape, .. } if *shape == e.shape) {
                    *prim = RPrim::Write {
                        shape: e.shape.clone(),
                        content: Resolved::plain(&snapshot),
                    };
                }
            }
        }
        let has_dir = rec.dirs.contains(&host.name().to_string());
        if !body.is_empty() {
            self.executors[idx]
                .run(&host, &id, &body)
                .map_err(|e| e.to_string())?;
        }
        if has_dir {
            let _ = self.executors[idx].remove_file(&host, &id, &format!("markers/{n}"));
        }
        Ok(report)
    }

    pub(crate) fn release(&mut self, rec: &mut InstanceRecord) -> Result<(), EngineError> {
        for lid in rec.ledger_ids.drain(..) {
            self.ledger = self.ledger.release(&lid);
        }
        Ok(self.store.write_ledger(&self.ledger)?)
    }

    // --- verbs ---------------------------------------------------------------

    /// `rue bootstrap <host>` (7.7): what the target has and, for what it
    /// lacks, the exact commands for its OS family. Nothing is run.
    pub fn bootstrap(&mut self, host: &str) -> Result<(BootstrapState, Vec<String>), EngineError> {
        let h = self
            .host_of(host)
            .ok_or_else(|| EngineError::NoSuchInstance(format!("no host record for {host}")))?;
        let ex = self
            .executor_for(&h)
            .ok_or_else(|| EngineError::Internal(format!("no executor reaches {host}")))?;
        let state = ex
            .bootstrap_state(&h)
            .map_err(|e| EngineError::Internal(format!("{host}: {e}")))?;
        let root = h.rue_root.clone().unwrap_or_else(|| {
            rue_render::Instance::default_root(rue_core::artifact::shell_of(&h.record.os))
                .to_string()
        });
        Ok((
            state.clone(),
            bootstrap_commands(&h.record.os, &root, &state),
        ))
    }

    /// `rue doctor`: every host's reach and bootstrap, the sinks, signing,
    /// settle, and the instances.
    pub fn doctor(&mut self) -> Result<DoctorReport, EngineError> {
        let mut hosts = Vec::new();
        let names: Vec<String> = self.hosts.keys().cloned().collect();
        for name in names {
            let h = self.hosts[&name].clone();
            let executor = self
                .executor_for(&h)
                .map(|e| e.locus().transport().to_string());
            let bootstrap = match self.executor_for(&h) {
                Some(ex) => ex.bootstrap_state(&h).ok(),
                None => None,
            };
            let scheduler_bound = match &h.scheduler {
                Some(n) => self.scheduler_named(&n.clone()).is_some(),
                None => false,
            };
            hosts.push(HostDoctor {
                name: name.clone(),
                executor,
                bootstrap,
                scheduler: h.scheduler.clone(),
                scheduler_bound,
            });
        }
        let orphans = self.orphans()?;
        Ok(DoctorReport {
            hosts,
            orphans,
            sinks: self.journal.sink_names(),
            signed: self.journal.signed(),
            settling: self.settling,
            instances: self.store.instance_ids()?.len(),
        })
    }

    /// Continue an `Applying` instance's walk (after boot or a satisfied
    /// wait); any other state is left as it is.
    pub fn advance(&mut self, id: &str) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        self.walk(&mut rec)?;
        Ok(self.outcome(&rec))
    }

    pub fn recant(&mut self, id: &str, force: &[ForceName]) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        if rec.state == State::DriftHeld && force.iter().any(|f| matches!(f, ForceName::Drift)) {
            self.step(&mut rec, E::ForceDrift)?;
            rec.force_drift = true;
            rec.drift_held.clear();
        } else {
            self.step(&mut rec, E::Recant)?;
        }
        for f in force {
            if let ForceName::Guard(g) = f {
                rec.forced.push(g.clone());
            }
        }
        self.log(&rec, J::Recant)?;
        self.revert(&mut rec)?;
        Ok(self.outcome(&rec))
    }

    pub fn renew(&mut self, id: &str, wane: Duration) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        self.step(&mut rec, E::Renew)?;
        let deadline = rec
            .deadline
            .ok_or_else(|| EngineError::Internal("a temporary plan with no deadline".into()))?;
        let within = rec.plan().renew_within.ok_or_else(|| {
            EngineError::NotAdmitted(RCode::R0102, "the plan declares no renew_within".into())
        })?;
        let now = self.clock.now();
        match states::renew(now, deadline, within, wane) {
            Ok(d) => {
                // The backstop is rearmed before the new expiry is the
                // instance's: a rearm that fails refuses the renewal.
                if let Err(why) = self.backstop_rearm(&mut rec, d)? {
                    self.log(
                        &rec,
                        J::Refused {
                            reason: why.clone(),
                        },
                    )?;
                    return Err(EngineError::Runtime(why));
                }
                rec.deadline = Some(d);
                self.log(&rec, J::Renewed)?;
                self.persist(&rec)?;
                Ok(self.outcome(&rec))
            }
            Err(states::RenewRefusal::Expired) => Err(EngineError::NotAdmitted(
                RCode::R0102,
                "the plan has expired; an expired plan is never renewed".into(),
            )),
            Err(states::RenewRefusal::OutsideWindow { until }) => Err(EngineError::NotAdmitted(
                RCode::R0102,
                format!("renewal opens at {}", until.unix_s),
            )),
        }
    }

    pub fn confirm(&mut self, id: &str) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        self.step(&mut rec, E::Confirm)?;
        if let Err(why) = self.backstop_disarm(&mut rec, Disarm::Confirmed)? {
            return Err(EngineError::Runtime(why));
        }
        self.log(&rec, J::Confirmed)?;
        self.persist(&rec)?;
        Ok(self.outcome(&rec))
    }

    pub fn commit(&mut self, id: &str, by: &str, reason: &str) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        self.step(&mut rec, E::CommitVerb)?;
        self.log(
            &rec,
            J::Committed {
                by: by.into(),
                reason: reason.into(),
            },
        )?;
        self.release(&mut rec)?;
        self.remove_dirs(&mut rec)?;
        self.persist(&rec)?;
        Ok(self.outcome(&rec))
    }

    pub fn resume(&mut self, id: &str, by: &str) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        let step = rec.held_at.unwrap_or(0);
        self.step(&mut rec, E::Resume)?;
        rec.held_at = None;
        self.log(
            &rec,
            J::Resumed {
                step,
                by: by.into(),
            },
        )?;
        self.walk(&mut rec)?;
        Ok(self.outcome(&rec))
    }

    pub fn handoff_done(&mut self, id: &str, step: u32, by: &str) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        let d = rec.deferred.clone();
        if d.as_ref().map(|d| d.step) != Some(step) {
            return Err(EngineError::WrongState {
                state: rec.state,
                verb: format!("handoff-done {step}"),
            });
        }
        self.step(&mut rec, E::HandoffDone)?;
        rec.deferred = None;
        rec.applied.push(AppliedStep { step, iteration: 0 });
        self.log(
            &rec,
            J::HandoffDone {
                step,
                by: by.into(),
            },
        )?;
        self.persist(&rec)?;
        self.walk(&mut rec)?;
        Ok(self.outcome(&rec))
    }

    pub fn abandon(&mut self, id: &str, by: &str, reason: &str) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        self.step(&mut rec, E::Abandon)?;
        let not_reverted: Vec<u32> = rec.applied.iter().map(|a| a.step).collect();
        // Disarm where the host is reachable; where it is not, the
        // artifact stays armed and the journal says so (7.7).
        let mut left_armed = Vec::new();
        if rec.backstop.as_ref().is_some_and(|b| b.installed) {
            if let Err(why) = self.backstop_disarm(&mut rec, Disarm::All)? {
                left_armed.push(why);
            }
        }
        self.log(
            &rec,
            J::Abandoned {
                steps_not_reverted: not_reverted,
                artifacts_left_armed: left_armed,
                by: by.into(),
                reason: reason.into(),
            },
        )?;
        rec.closed_reason = Some(format!("abandoned by {by}: {reason}"));
        self.drop_secrets(&rec, "abandoned")?;
        self.log(
            &rec,
            J::Closed {
                reason: rec.closed_reason.clone().unwrap_or_default(),
            },
        )?;
        self.release(&mut rec)?;
        self.persist(&rec)?;
        Ok(self.outcome(&rec))
    }

    pub fn cancel(&mut self, id: &str) -> Result<Outcome, EngineError> {
        let mut rec = self.load(id)?;
        self.step(&mut rec, E::Cancel)?;
        self.log(&rec, J::Cancelled)?;
        self.drop_secrets(&rec, "cancelled")?;
        rec.closed_reason = Some("cancelled".into());
        self.log(
            &rec,
            J::Closed {
                reason: "cancelled".into(),
            },
        )?;
        self.release(&mut rec)?;
        self.persist(&rec)?;
        Ok(self.outcome(&rec))
    }

    // --- the reap pass -------------------------------------------------------

    pub fn reap(&mut self) -> Result<ReapReport, EngineError> {
        let mut report = ReapReport {
            settling: self.settling,
            ..ReapReport::default()
        };
        let now = self.clock.now();
        for line in self.expire_secrets()? {
            report.actions.push(line);
        }
        for rec in self.instances()? {
            let mut rec = rec;
            if states::terminal(rec.state) {
                // An abandoned instance whose artifact was left armed: a
                // later firing is accepted and visible (7.7).
                if self.backstop_fired_after_abandon(&mut rec)? {
                    report
                        .actions
                        .push(format!("{}: the backstop fired after abandon", rec.id));
                }
                continue;
            }
            // Pending: a wait factor may have accrued enough weight to
            // open the gate without another proof (5.11).
            if rec.state == State::Pending {
                if let Some(g) = rec.plan().gate.as_ref().map(|g| g.expr.clone()) {
                    if self.gate_satisfied(&rec, Scope::Plan, &g) {
                        self.approve(&mut rec)?;
                        self.persist(&rec)?;
                        self.walk(&mut rec)?;
                        report
                            .actions
                            .push(format!("{}: the plan gate opened", rec.id));
                        continue;
                    }
                }
            }
            // Pending: the approval window.
            if rec.state == State::Pending {
                if let Some(d) = rec.approval_deadline {
                    if states::expired(now, d) {
                        self.step(&mut rec, E::ApprovalWindowLapses)?;
                        self.log(&rec, J::ApprovalExpired)?;
                        self.step(&mut rec, E::Cancel)?;
                        rec.closed_reason = Some("approval window lapsed".into());
                        self.log(
                            &rec,
                            J::Closed {
                                reason: "approval window lapsed".into(),
                            },
                        )?;
                        self.release(&mut rec)?;
                        self.persist(&rec)?;
                        report
                            .actions
                            .push(format!("{}: approval window lapsed", rec.id));
                    }
                }
                continue;
            }
            if rec.state == State::ApprovalExpired {
                self.step(&mut rec, E::Cancel)?;
                rec.closed_reason = Some("approval window lapsed".into());
                self.log(
                    &rec,
                    J::Closed {
                        reason: "approval window lapsed".into(),
                    },
                )?;
                self.release(&mut rec)?;
                self.persist(&rec)?;
                report
                    .actions
                    .push(format!("{}: reaped after the approval window", rec.id));
                continue;
            }
            // What a fired artifact left, read on this contact (R0402).
            if self.backstop_read_fired(&mut rec)? {
                report
                    .actions
                    .push(format!("{}: the backstop fired on its target", rec.id));
            }
            if matches!(rec.state, State::DriftHeld | State::Stuck)
                || (rec.permanent && matches!(rec.state, State::Held | State::Deferred))
            {
                report.notify.push((rec.id.clone(), rec.state));
                // Unbounded states are re-notified every pass: what ends
                // them is a person, and one message can be missed (5.9).
                if let Some(n) = self.notify.as_mut() {
                    let _ = n.deliver(
                        Level::Blocked,
                        &rec.id,
                        &format!(
                            "{} since {}: only an operator ends this",
                            rec.state,
                            rec.requested_at.map(|i| i.unix_s).unwrap_or(0)
                        ),
                    );
                }
            }
            if self.settling {
                continue;
            }
            // Wane: every bounded state of a temporary plan, observed before
            // anything else the pass does to the instance.
            if let Some(d) = rec.deadline {
                if states::expired(now, d) {
                    match states::transition(rec.ctx(), rec.state, E::WaneElapses) {
                        Verdict_::To(State::Expired) => {
                            self.step(&mut rec, E::WaneElapses)?;
                            self.log(&rec, J::Expired)?;
                            rec.refusal = None;
                            self.revert(&mut rec)?;
                            report.actions.push(format!("{}: wane elapsed", rec.id));
                            continue;
                        }
                        // DriftHeld, Stuck and Expired: observed, unchanged
                        // (rule 3); the notification above is the response.
                        Verdict_::Stay => {
                            self.step(&mut rec, E::WaneElapses)?;
                        }
                        _ => {}
                    }
                }
            }
            // An undo the engine was in the middle of when it stopped.
            if matches!(rec.state, State::Reverting | State::Expired) {
                self.revert(&mut rec)?;
                report.actions.push(format!("{}: undo continued", rec.id));
                continue;
            }
            match rec.state {
                State::Waiting => {
                    let w = rec.waiting.clone().unwrap_or(Wait {
                        step: 0,
                        reason: String::new(),
                        since: now,
                        bound: None,
                        guard: None,
                    });
                    if w.bound.is_some_and(|b| states::expired(now, b)) {
                        self.log(
                            &rec,
                            J::WaitLapsed {
                                step: w.step,
                                reason: w.reason.clone(),
                            },
                        )?;
                        self.step(&mut rec, E::BoundLapses)?;
                        rec.waiting = None;
                        if rec.state == State::Held {
                            rec.held_at = Some(w.step);
                            self.log(&rec, J::Held { step: w.step })?;
                            self.persist(&rec)?;
                        } else {
                            rec.refusal =
                                Some(format!("step {}: wait lapsed ({})", w.step, w.reason));
                            self.revert(&mut rec)?;
                        }
                        report
                            .actions
                            .push(format!("{}: wait at step {} lapsed", rec.id, w.step));
                    } else if let Some(g) = &w.guard {
                        let owner = self.owner_host(&rec)?;
                        match self.observe(&rec, &owner, g) {
                            Ok(Tri::Yes) => {
                                self.step(&mut rec, E::WaitSatisfied)?;
                                rec.waiting = None;
                                self.log(&rec, J::StepGateSatisfied { step: w.step })?;
                                self.persist(&rec)?;
                                self.walk(&mut rec)?;
                                report
                                    .actions
                                    .push(format!("{}: guard {g} now yes", rec.id));
                            }
                            Ok(Tri::No) => {
                                self.step(&mut rec, E::WaitSatisfied)?;
                                rec.waiting = None;
                                self.refuse(&mut rec, w.step, &format!("guard {g} is no"))?;
                                report.actions.push(format!("{}: guard {g} now no", rec.id));
                            }
                            _ => {}
                        }
                    }
                }
                State::Stuck => {
                    self.step(&mut rec, E::Retry)?;
                    self.revert(&mut rec)?;
                    report.actions.push(format!("{}: retried the undo", rec.id));
                }
                State::Deferred => {
                    if let Some(d) = rec.deferred.clone() {
                        if let Some(op) = rec.op_at(d.step).cloned() {
                            if let Some(probe) = &op.handoff_done {
                                let owner = self.owner_host(&rec)?;
                                if let Ok(Tri::Yes) = self.observe(&rec, &owner, probe) {
                                    let id = rec.id.clone();
                                    self.handoff_done(&id, d.step, "probe")?;
                                    report
                                        .actions
                                        .push(format!("{}: handoff done by probe {probe}", id));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(report)
    }

    // --- boot recovery and settle --------------------------------------------

    /// Boot recovery (7.8): demote `Applying` to `Reverting` and revert;
    /// reestablish every `Held` resource of an applied step or record it
    /// lost; re-observe every owned footprint; only then leave settle, during
    /// which no wane fires and no stuck retry runs.
    pub fn boot(&mut self) -> Result<BootReport, EngineError> {
        let mut report = BootReport::default();
        self.set_settling(true)?;
        let migrated = self.store.read_meta("migrated")?;
        if let (Some(from), Some(to), Some(by)) =
            (migrated.get("from"), migrated.get("to"), migrated.get("by"))
        {
            let (f, t) = (
                from.parse().unwrap_or(0),
                to.parse().unwrap_or(crate::store::SCHEMA),
            );
            let about = About {
                plan: String::new(),
                instance: String::new(),
                host: String::new(),
            };
            let at = self.clock.now();
            self.journal.record(
                &self.store,
                at,
                &about,
                J::Migrated {
                    from: f,
                    to: t,
                    by: by.clone(),
                },
                Vec::new(),
            )?;
            self.store.write_meta("migrated", &BTreeMap::new())?;
            report.migrated = Some((f, t));
        }
        for rec in self.instances()? {
            let mut rec = rec;
            if rec.state == State::Applying {
                // The step whose `do` was in flight may have half
                // happened; its write-ahead entry says how to undo it, so
                // it is undone with the rest (5.9). An undo of a step that
                // never took is harmless: that is what makes an undo an
                // undo.
                if let Some(a) = rec.attempting.take() {
                    if !rec.is_applied(a.step, a.iteration) {
                        report.interrupted.push((rec.id.clone(), a.step));
                        rec.applied.push(a);
                    }
                }
                rec.refusal = Some("the engine restarted while applying".into());
                self.step(&mut rec, E::Refuse)?;
                if rec.state == State::Held {
                    rec.held_at = rec.applied.last().map(|a| a.step);
                    self.log(
                        &rec,
                        J::Held {
                            step: rec.held_at.unwrap_or(0),
                        },
                    )?;
                    self.persist(&rec)?;
                } else {
                    self.revert(&mut rec)?;
                }
                report.demoted.push(rec.id.clone());
                continue;
            }
            if states::terminal(rec.state) {
                continue;
            }
            self.remove_staged(&mut rec, None, "boot recovery")?;
            // Held resources of applied steps.
            for a in rec.applied.clone() {
                let op = match rec.op_at(a.step) {
                    Some(o) => o.clone(),
                    None => continue,
                };
                if !op.footprint.iter().any(|e| e.kind == Kind::Held) {
                    continue;
                }
                let ok = match &op.reestablish {
                    Some(body) => self.run_body(&rec, &op, body).is_ok(),
                    None => false,
                };
                if ok {
                    if rec.state == State::Suspended {
                        self.step(&mut rec, E::Reestablish)?;
                    }
                    self.log(&rec, J::Reestablished)?;
                    report.reestablished.push((rec.id.clone(), a.step));
                } else {
                    if states::transition(rec.ctx(), rec.state, E::Suspend)
                        == Verdict_::To(State::Suspended)
                    {
                        self.step(&mut rec, E::Suspend)?;
                        self.log(&rec, J::Suspended)?;
                    }
                    report.lost.push((rec.id.clone(), a.step));
                }
            }
            // Owned footprints re-observed.
            for a in rec.applied.clone() {
                let op = match rec.op_at(a.step) {
                    Some(o) => o.clone(),
                    None => continue,
                };
                let host = match self.step_host(&rec, &op) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                for e in op
                    .footprint
                    .iter()
                    .filter(|e| matches!(e.kind, Kind::Owned | Kind::Region))
                {
                    if let Some(ex) = self.executor_for(&host) {
                        let _ = ex.observe(
                            &host,
                            &ProbeRun {
                                name: e.shape.clone(),
                                body: Vec::new(),
                            },
                        );
                    }
                    report.reobserved.push((rec.id.clone(), e.shape.clone()));
                }
            }
        }
        report.secrets_dropped = self.drop_secrets_at_boot()?;
        let (orphaned, reclaimed) = self.reconcile()?;
        report.orphaned = orphaned;
        report.reclaimed = reclaimed;
        self.set_settling(false)?;
        Ok(report)
    }

    fn run_body(&mut self, rec: &InstanceRecord, op: &Op, body: &Body) -> Result<(), String> {
        let host = self.step_host(rec, op)?;
        let env = self.env_for(rec, &BTreeMap::new());
        let rb = resolve_body(body, &host, &env).map_err(|u| u.to_string())?;
        let id = rec.id.clone();
        let ex = self
            .executor_for(&host)
            .ok_or_else(|| format!("no executor reaches {}", host.name()))?;
        ex.run(&host, &id, &rb)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

pub(crate) enum Outcome_ {
    Moved,
    Stayed,
}

/// What an undo found: facts clobbered under `:clobber`, facts held under
/// `:defer` (the step then stays applied and the instance is DriftHeld).
#[derive(Debug, Default)]
struct UndoReport {
    clobbered: Vec<String>,
    held: Vec<String>,
}

fn leaf_count(items: &[Item]) -> u32 {
    numbered(items).len() as u32
}

/// The umbra each host reserves at request (5.12): the facts every leaf
/// writes or may write, grouped by the leaf's host.
fn umbras(p: &Plan) -> Vec<(String, Vec<rue_core::interference::Fact>)> {
    let mut by_host: BTreeMap<String, Vec<rue_core::interference::Fact>> = BTreeMap::new();
    for leaf in step_facts(&p.owner, &p.body) {
        let e = by_host.entry(leaf.host.clone()).or_default();
        e.extend(writes(leaf.op));
        e.extend(maywrite(leaf.op));
    }
    by_host.into_iter().collect()
}

/// The body a `:restore` undo runs, from the footprint and the snapshots
/// taken before `do`: an owned file is removed, a region stripped, a
/// modified file written back. A non-file fact under restore is the
/// executor's to know how to restore; the engine has nothing to restore it
/// from, and says so rather than guessing.
pub fn restore_body(
    n: u32,
    footprint: &[FootprintEntry],
    snapshots: &BTreeMap<String, String>,
) -> Result<Vec<RPrim>, String> {
    let mut body = Vec::new();
    for (k, e) in footprint.iter().enumerate() {
        match e.kind {
            Kind::Owned => body.push(RPrim::Remove {
                shape: e.shape.clone(),
            }),
            Kind::Region => body.push(RPrim::RegionClear {
                shape: e.shape.clone(),
                anchor: e.anchor.clone(),
            }),
            Kind::Modified => match snapshots.get(&format!("{n}/{k}")) {
                Some(s) => body.push(RPrim::Write {
                    shape: e.shape.clone(),
                    content: Resolved::plain(s),
                }),
                None if e.shape.starts_with("file:") => body.push(RPrim::Remove {
                    shape: e.shape.clone(),
                }),
                None => {
                    return Err(format!(
                        "restore of {} has no snapshot and is not a file",
                        e.shape
                    ))
                }
            },
            Kind::Held => body.push(RPrim::Release {
                name: e.shape.clone(),
            }),
            Kind::Derived | Kind::AppendOnly => {}
        }
    }
    Ok(body)
}

/// The steps a record has applied, for tests and `rue status`.
pub fn applied_steps(rec: &InstanceRecord) -> BTreeSet<u32> {
    rec.applied.iter().map(|a| a.step).collect()
}
