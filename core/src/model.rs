//! The core model, docs/ROADMAP.md sections 5.1 to 5.4, and its serde shape,
//! which is the plan IR (docs/TESTING.md, "The plan IR").
//!
//! Names are the roadmap's; the simplifications are the prototype's and are
//! named: bodies are opaque here (`undo_closed` and `undo_idempotent` stand
//! in for the analyses of the second Phase 1 unit), facts are named by shape
//! strings, and guards carry a fixed [`Tri`] so a plan's verdict can be
//! computed without a world.
//!
//! The IR spelling: durations are whole seconds under names ending in `_s`;
//! a unit variant is a bare string, a data-carrying variant a one-key object;
//! an item carries an `item` tag with a step's fields flattened beside it.
//! Every struct refuses unknown fields, so an emitter that grows a field
//! without bumping `ir_version` fails loudly here.

use serde::{Deserialize, Serialize};

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

/// The undo. `Computed` and `Compensate` carry their declared `undo_pre` as
/// the shapes the undo needs unchanged; `Restore` derives it from the
/// footprint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Undo {
    Restore,
    Computed(Vec<String>),
    Compensate(Vec<String>),
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
    /// `suspend:` and `reestablish:` defined.
    pub has_suspend: bool,
    /// The probe that continues a deferred step.
    pub handoff_done: Option<String>,
    /// Phase 0 stand-in for the closure analysis.
    pub undo_closed: bool,
    /// Phase 0 stand-in for the idempotency analysis.
    pub undo_idempotent: bool,
    /// The undo line `explain` prints.
    pub undo_one_line: String,
}

impl Op {
    /// An op with every optional field at its quiet default.
    pub fn new(id: &str, footprint: Footprint) -> Op {
        Op {
            id: id.to_string(),
            footprint,
            pre: Vec::new(),
            undo: Undo::Restore,
            post: Vec::new(),
            undo_locus: UndoLocus::Controller,
            refusal: Refusal::Revert,
            drift: None,
            reach: Vec::new(),
            outputs: Vec::new(),
            exclusivity: None,
            locus: Locus::Target,
            has_suspend: false,
            handoff_done: None,
            undo_closed: true,
            undo_idempotent: true,
            undo_one_line: "restore".to_string(),
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
