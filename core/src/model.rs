//! The core model, docs/ROADMAP.md sections 5.1 to 5.4, and its serde shape,
//! which is the plan IR (docs/TESTING.md, "The plan IR").
//!
//! Names are the roadmap's; the simplifications are named: facts are named by
//! shape strings, guards carry a fixed [`Tri`] so a plan's verdict can be
//! computed without a world, and `undo_idempotent` stands in for an analysis
//! that is not Phase 1's. Bodies are [`crate::body`]'s primitives.
//!
//! The IR spelling: durations are whole seconds under names ending in `_s`;
//! a unit variant is a bare string, a data-carrying variant a one-key object;
//! an item carries an `item` tag with a step's fields flattened beside it.
//! Every struct refuses unknown fields, so an emitter that grows a field
//! without bumping `ir_version` fails loudly here.

use serde::{Deserialize, Serialize};

use crate::body::Body;

/// A host name.
pub type Host = String;

/// Whole seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Duration {
    pub seconds: u64,
}

impl Duration {
    pub const fn new(seconds: u64) -> Duration {
        Duration { seconds }
    }

    /// The surface spelling: the largest unit that divides evenly, else
    /// seconds (`14400` is `4h`, `600` is `10m`, `90` is `90s`, `0` is `0s`).
    pub fn render(self) -> String {
        let s = self.seconds;
        if s != 0 && s.is_multiple_of(86_400) {
            format!("{}d", s / 86_400)
        } else if s != 0 && s.is_multiple_of(3_600) {
            format!("{}h", s / 3_600)
        } else if s != 0 && s.is_multiple_of(60) {
            format!("{}m", s / 60)
        } else {
            format!("{s}s")
        }
    }
}

/// A point in time, whole seconds since the Unix epoch. Core never reads a
/// clock; every `now` is an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Instant {
    pub unix_s: u64,
}

impl Instant {
    pub const fn new(unix_s: u64) -> Instant {
        Instant { unix_s }
    }

    pub fn plus(self, d: Duration) -> Instant {
        Instant {
            unix_s: self.unix_s + d.seconds,
        }
    }
}

/// Three-valued truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tri {
    Yes,
    No,
    Unknown,
}

// ---------------------------------------------------------------------------
// Facts and footprints (5.2)

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Owned,
    Region,
    Modified,
    Derived,
    AppendOnly,
    Held,
}

impl Kind {
    /// The default drift policy by footprint kind (section 5.2).
    pub fn default_drift(self) -> Option<Drift> {
        match self {
            Kind::Owned | Kind::Region => Some(Drift::Clobber),
            Kind::Modified => Some(Drift::Defer),
            Kind::Derived | Kind::AppendOnly | Kind::Held => None,
        }
    }
}

/// A footprint entry: the shape is static and names the fact; the instance,
/// when bound, is what a value flowing in made concrete; the anchor is a
/// region's fence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintEntry {
    pub kind: Kind,
    pub shape: String,
    pub instance: Option<String>,
    pub anchor: Option<String>,
}

impl FootprintEntry {
    pub fn entry(kind: Kind, shape: &str) -> FootprintEntry {
        FootprintEntry {
            kind,
            shape: shape.to_string(),
            instance: None,
            anchor: None,
        }
    }

    pub fn anchored(shape: &str, anchor: &str) -> FootprintEntry {
        FootprintEntry {
            kind: Kind::Region,
            shape: shape.to_string(),
            instance: None,
            anchor: Some(anchor.to_string()),
        }
    }
}

pub type Footprint = Vec<FootprintEntry>;

// ---------------------------------------------------------------------------
// Guards (5.1)

/// A guard is an expression yielding a [`Tri`]. There is no evaluator here,
/// so a guard carries the value a check should assume, its name (what
/// `force:` refers to), and whether it declares `force: never`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guard {
    pub name: String,
    pub value: Tri,
    pub force_never: bool,
}

impl Guard {
    pub fn new(name: &str, value: Tri) -> Guard {
        Guard {
            name: name.to_string(),
            value,
            force_never: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Ops (5.3)

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRef {
    Static(Host),
    /// Bound at runtime from a named output.
    Bound(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locus {
    Controller,
    Target,
    Host(HostRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoLocus {
    Target,
    Controller,
    #[serde(rename = "none")]
    NoLocus,
}

/// The undo. `Computed` and `Compensate` carry a body and their declared
/// `undo_pre`, the shapes the undo needs unchanged; `Restore` derives both
/// from the footprint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Undo {
    Restore,
    Computed {
        body: Body,
        undo_pre: Vec<String>,
    },
    Compensate {
        body: Body,
        undo_pre: Vec<String>,
    },
    #[serde(rename = "none")]
    NoUndo,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cost {
    Probe(String),
    /// Declared `:none`, with the reason.
    #[serde(rename = "none")]
    NoCost(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ack {
    Gate(GateExpr),
    /// Declared `:none`, with the reason.
    #[serde(rename = "none")]
    NoAck(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Revert,
    Hold {
        /// The `hold_via:` op, if any.
        via: Option<String>,
    },
    Knell {
        guard: Option<Guard>,
        cost: Cost,
        ack: Ack,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Drift {
    Clobber,
    Defer,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub name: String,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Op {
    pub id: String,
    pub footprint: Footprint,
    pub pre: Vec<Guard>,
    #[serde(rename = "do")]
    pub do_: Body,
    pub undo: Undo,
    pub post: Vec<Guard>,
    pub undo_locus: UndoLocus,
    pub refusal: Refusal,
    /// `None` means the kind's default.
    pub drift: Option<Drift>,
    /// Transports this op may sever.
    pub reach: Vec<String>,
    pub outputs: Vec<Output>,
    pub exclusivity: Option<String>,
    pub locus: Locus,
    /// Required together iff a `Held` footprint (E0205).
    pub suspend: Option<Body>,
    pub reestablish: Option<Body>,
    /// The probe that continues a deferred step.
    pub handoff_done: Option<String>,
    /// Stand-in for the idempotency analysis, which is not Phase 1's (E0208).
    pub undo_idempotent: bool,
}

impl Op {
    /// An op with every optional field at its quiet default.
    pub fn new(id: &str, footprint: Footprint) -> Op {
        Op {
            id: id.to_string(),
            footprint,
            pre: Vec::new(),
            do_: Vec::new(),
            undo: Undo::Restore,
            post: Vec::new(),
            undo_locus: UndoLocus::Controller,
            refusal: Refusal::Revert,
            drift: None,
            reach: Vec::new(),
            outputs: Vec::new(),
            exclusivity: None,
            locus: Locus::Target,
            suspend: None,
            reestablish: None,
            handoff_done: None,
            undo_idempotent: true,
        }
    }

    /// The drift policy in force: the declared one, else the first default
    /// any footprint kind supplies.
    pub fn effective_drift(&self) -> Option<Drift> {
        match self.drift {
            Some(d) => Some(d),
            None => self.footprint.iter().find_map(|e| e.kind.default_drift()),
        }
    }
}

// ---------------------------------------------------------------------------
// Gates (5.11)

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authenticator {
    pub id: String,
    pub human: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateExpr {
    Thresh { n: u32, factors: Vec<Factor> },
    Single(Factor),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Factor {
    Auth {
        id: String,
        weight: u32,
    },
    Humans {
        weight: u32,
    },
    Group {
        expr: Box<GateExpr>,
        weight: u32,
    },
    Wait {
        #[serde(rename = "duration_s")]
        duration: Duration,
        weight: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanGate {
    pub expr: GateExpr,
    #[serde(rename = "window_s")]
    pub window: Option<Duration>,
    pub allow_zero_human: bool,
}

// ---------------------------------------------------------------------------
// Plans and items (5.4)

/// Which way a step runs. Reversal flips it; that is all reversal is,
/// syntactically, and it is what makes `reverse . reverse = id` a law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Forward,
    Inverse,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForceName {
    Guard(String),
    Drift,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnLapse {
    Revert,
    Hold,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepI {
    pub op: Op,
    pub direction: Direction,
    pub gate: Option<GateExpr>,
    #[serde(rename = "window_s")]
    pub window: Option<Duration>,
    pub on_lapse: OnLapse,
    pub force: Vec<ForceName>,
    pub alias: Option<String>,
    /// Rendered arguments, for `explain`.
    pub args: Vec<String>,
}

impl StepI {
    pub fn new(op: Op) -> StepI {
        StepI {
            op,
            direction: Direction::Forward,
            gate: None,
            window: None,
            on_lapse: OnLapse::Revert,
            force: Vec::new(),
            alias: None,
            args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatForm {
    Count(u32),
    Over {
        /// The list expression.
        list: String,
        /// The literal cap.
        max: u32,
        set_valued: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "item", rename_all = "snake_case")]
pub enum Item {
    Step(StepI),
    /// The op must have `refusal: Knell` (E0201).
    Knell(StepI),
    Par {
        children: Vec<Item>,
    },
    Slot {
        name: String,
    },
    Confirm,
    Commit,
    Preflight {
        guards: Vec<Guard>,
    },
    Observe {
        probe: String,
        alias: String,
    },
    Assert {
        guard: Guard,
        #[serde(rename = "window_s")]
        window: Option<Duration>,
        on_lapse: OnLapse,
    },
    Repeat {
        form: RepeatForm,
        var: String,
        body: Vec<Item>,
    },
    When {
        guard: Guard,
        #[serde(rename = "window_s")]
        window: Option<Duration>,
        on_lapse: OnLapse,
        #[serde(rename = "then")]
        then_: Vec<Item>,
        #[serde(rename = "else")]
        else_: Vec<Item>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    After(Duration),
    UnlessConfirmed(Duration),
    UnlessHeartbeat {
        #[serde(rename = "deadline_s")]
        deadline: Duration,
        #[serde(rename = "interval_s")]
        interval: Option<Duration>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backstop {
    pub triggers: Vec<Trigger>,
    /// Arming happens before this step number; `last + 1` is late arming
    /// after the last covered step.
    pub arm_before: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strictness {
    Strict,
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Manual,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalRequirement {
    Chained,
    Signed,
}

/// A probe declaration (section 5.1) as the engine runs it: its body on
/// its locus produces facts. A probe's command answers a guard by its exit
/// status (0 yes, 1 no, anything else unknown) and its stdout is the
/// fact's value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeDecl {
    pub name: String,
    pub locus: Locus,
    pub body: Body,
    pub produces: Vec<String>,
    /// A host-contract fact, frozen at request (5.1).
    #[serde(rename = "static")]
    pub static_: bool,
    /// The notion of "restored" for the facts it produces: `bytes`,
    /// `line_set`, `json`, or a tenant-declared name.
    pub equivalence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub id: String,
    pub owner: Host,
    pub gate: Option<PlanGate>,
    #[serde(rename = "wane_s")]
    pub wane: Option<Duration>,
    #[serde(rename = "renew_within_s")]
    pub renew_within: Option<Duration>,
    pub backstop: Option<Backstop>,
    pub fires_by_construction: bool,
    pub strictness: Strictness,
    pub mode: Mode,
    pub exclusivity: Option<String>,
    pub require_journal: Option<JournalRequirement>,
    pub body: Vec<Item>,
    /// The probes the plan's guards, observes and asserts may name (IR 4).
    #[serde(default)]
    pub probes: Vec<ProbeDecl>,
}

impl Plan {
    pub fn new(id: &str, owner: &str, body: Vec<Item>) -> Plan {
        Plan {
            id: id.to_string(),
            owner: owner.to_string(),
            gate: None,
            wane: None,
            renew_within: None,
            backstop: None,
            fires_by_construction: false,
            strictness: Strictness::Strict,
            mode: Mode::Manual,
            exclusivity: None,
            require_journal: None,
            body,
            probes: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// The world the checker is handed

/// The parts of a host record a check reads: whether an executor can reach
/// it at all, and whether that executor has a filesystem (an instance
/// directory can live there; a `:target` undo is possible).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRecord {
    pub name: Host,
    pub os: String,
    pub reach: Vec<String>,
    pub filesystem: bool,
    /// The executor honours the stdin preamble that carries `env:` and
    /// `stdin:` secrets (section 7.4); false for an API appliance.
    pub stdin_preamble: bool,
    /// The language the host's backstop artifact is rendered in; `None`
    /// is the host's native shell (`crate::artifact::default_language`).
    pub artifact: Option<ArtifactLanguage>,
}

/// A backstop artifact's language (sections 4.5 and 7.7): the host's native
/// shell, or Python run by `uv` with PEP 723 metadata, on any OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactLanguage {
    Sh,
    Powershell,
    Python,
}

impl ArtifactLanguage {
    /// The name as the surface and the diagnostics spell it.
    pub fn name(self) -> &'static str {
        match self {
            ArtifactLanguage::Sh => "sh",
            ArtifactLanguage::Powershell => "powershell",
            ArtifactLanguage::Python => "python",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub hosts: Vec<HostRecord>,
    /// Transports the declared executors serve.
    pub transports: Vec<String>,
    pub authenticators: Vec<Authenticator>,
    #[serde(rename = "max_wait_s")]
    pub max_wait: Option<Duration>,
    /// Hosts whose scheduler binding reports presence.
    pub scheduler_present: Vec<Host>,
    /// The `secrets deliver_to:` acceptors, in order (E0606 when empty and a
    /// plan has a secret output).
    pub secrets_deliver_to: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_render_in_the_largest_even_unit() {
        assert_eq!(Duration::new(14_400).render(), "4h");
        assert_eq!(Duration::new(1_800).render(), "30m");
        assert_eq!(Duration::new(600).render(), "10m");
        assert_eq!(Duration::new(90).render(), "90s");
        assert_eq!(Duration::new(172_800).render(), "2d");
        assert_eq!(Duration::new(0).render(), "0s");
    }

    #[test]
    fn effective_drift_is_declared_else_the_first_kind_default() {
        let mut o = Op::new(
            "a",
            vec![
                FootprintEntry::entry(Kind::Derived, "probe:x"),
                FootprintEntry::entry(Kind::Modified, "file:/a"),
            ],
        );
        assert_eq!(o.effective_drift(), Some(Drift::Defer));
        o.drift = Some(Drift::Clobber);
        assert_eq!(o.effective_drift(), Some(Drift::Clobber));
        assert_eq!(Op::new("b", vec![]).effective_drift(), None);
    }
}
