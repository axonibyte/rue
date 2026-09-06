//! The verdict's structured form, docs/ROADMAP.md section 5.8, and its
//! canonical JSON. The prose is [`crate::prose`]; `explain` is
//! [`crate::explain`]. Every "the verdict says" in the roadmap names a field
//! here; a new clause without a field is a schema bump. The JSON is written
//! field by field, as the prototype's is, so the bytes are the specification's
//! and not a derive's.

use serde_json::{json, Map, Value};

use crate::diagnostics::Code;
use crate::intent::Intent;
use crate::model::Duration;

pub const VERDICT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub step: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointOfNoReturn {
    pub step: u32,
    pub guard: Option<String>,
    pub cost: String,
    pub ack: String,
    pub gate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateVerdict {
    pub satisfiable: bool,
    pub min_distinct_humans: Option<usize>,
    pub zero_human_path: bool,
    pub window: Option<Duration>,
    pub wait_alone_at: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackstopVerdict {
    pub triggers: Vec<String>,
    pub covers: Vec<u32>,
    pub installed_before: Option<u32>,
    /// Armed before this step, or
    pub armed_before: Option<u32>,
    /// armed after this step (late arming).
    pub armed_after: Option<u32>,
    pub late_arming_window: Vec<u32>,
    pub scheduler: String,
    pub granularity: Duration,
    pub self_enforced: bool,
    pub drift_policy: Vec<(u32, String)>,
    pub snapshot_location: String,
    pub snapshot_cap: u64,
    pub conditional: Vec<(u32, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostTouched {
    /// A host and the directory its markers live in (`target` or `controller`).
    Host { host: String, directory: String },
    /// Where the binding comes from.
    Unresolved(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    pub source: String,
    pub host_contract_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayConflict {
    pub earlier: u32,
    pub later: u32,
    pub fact: String,
    pub refused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepGateVerdict {
    pub expr: String,
    pub window: Option<Duration>,
    pub wait_alone_at: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnellVerdict {
    pub guard: Option<String>,
    pub cost: String,
    pub ack: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepVerdict {
    pub n: u32,
    pub op: String,
    pub locus: String,
    pub undo: Option<String>,
    pub undo_locus: String,
    pub refusal: String,
    pub drift: Option<String>,
    pub gate: Option<StepGateVerdict>,
    pub knell: Option<KnellVerdict>,
    pub conditional: Option<String>,
    pub footprint: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub plan: String,
    pub host: String,
    pub status: Status,
    pub intent: Option<Intent>,
    pub rehearsal: bool,
    /// `manual` or `auto`: which hold and acknowledgement rules applied.
    pub mode: String,
    pub commit_step: Option<u32>,
    pub fires_by_construction: bool,
    pub wane: Option<Duration>,
    pub reversible_through: u32,
    pub holds_at: Vec<u32>,
    pub point_of_no_return: Option<PointOfNoReturn>,
    pub reversible_back_to: Option<(u32, u32)>,
    pub gate: Option<GateVerdict>,
    pub backstop: Option<BackstopVerdict>,
    pub controller_only_undos: Vec<u32>,
    pub held_indefinitely: Vec<u32>,
    pub induced_defer: Vec<u32>,
    pub hosts_touched: Vec<(u32, Vec<HostTouched>)>,
    pub deferred: Vec<u32>,
    pub dispatch: Dispatch,
    pub may_conflicts: Vec<MayConflict>,
    pub unresolved_bindings: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub steps: Vec<StepVerdict>,
}

fn dur(d: Option<Duration>) -> Value {
    match d {
        Some(d) => json!(d.seconds),
        None => Value::Null,
    }
}

fn txt(t: &Option<String>) -> Value {
    match t {
        Some(s) => Value::String(s.clone()),
        None => Value::Null,
    }
}

fn opt_u32(n: Option<u32>) -> Value {
    match n {
        Some(n) => json!(n),
        None => Value::Null,
    }
}

fn step_keyed(pairs: impl Iterator<Item = (u32, Value)>) -> Value {
    let mut m = Map::new();
    for (n, v) in pairs {
        m.insert(n.to_string(), v);
    }
    Value::Object(m)
}

pub fn to_json(v: &Verdict) -> Value {
    json!({
        "verdict_version": VERDICT_VERSION,
        "plan": v.plan,
        "host": v.host,
        "status": match v.status { Status::Ok => "ok", Status::Refused => "refused" },
        "intent": match v.intent { Some(Intent::Temporary) => json!("temporary"), Some(Intent::Permanent) => json!("permanent"), None => Value::Null },
        "rehearsal": v.rehearsal,
        "mode": v.mode,
        "commit_step": opt_u32(v.commit_step),
        "fires_by_construction": v.fires_by_construction,
        "wane_s": dur(v.wane),
        "reversible_through": v.reversible_through,
        "holds_at": v.holds_at,
        "point_of_no_return": match &v.point_of_no_return {
            Some(p) => json!({ "step": p.step, "guard": txt(&p.guard), "cost": p.cost, "ack": p.ack, "gate": txt(&p.gate) }),
            None => Value::Null,
        },
        "reversible_back_to": match v.reversible_back_to {
            Some((f, t)) => json!({ "from": f, "to": t }),
            None => Value::Null,
        },
        "gate": match &v.gate {
            Some(g) => json!({
                "satisfiable": g.satisfiable,
                "min_distinct_humans": match g.min_distinct_humans { Some(n) => json!(n), None => Value::Null },
                "zero_human_path": g.zero_human_path,
                "window_s": dur(g.window),
                "wait_alone_at_s": dur(g.wait_alone_at),
            }),
            None => Value::Null,
        },
        "backstop": match &v.backstop {
            Some(b) => json!({
                "triggers": b.triggers,
                "covers": b.covers,
                "locus": "target",
                "installed_before": opt_u32(b.installed_before),
                "armed_before": opt_u32(b.armed_before),
                "armed_after": opt_u32(b.armed_after),
                "late_arming_window": b.late_arming_window,
                "scheduler": b.scheduler,
                "granularity_s": b.granularity.seconds,
                "self_enforced": b.self_enforced,
                "drift_policy": step_keyed(b.drift_policy.iter().map(|(n, p)| (*n, Value::String(p.clone())))),
                "snapshots": { "location": b.snapshot_location, "cap_bytes": b.snapshot_cap },
                "conditional": b.conditional.iter().map(|(n, c)| json!({ "step": n, "on": c })).collect::<Vec<_>>(),
            }),
            None => Value::Null,
        },
        "controller_only_undos": v.controller_only_undos,
        "held_indefinitely": v.held_indefinitely,
        "induced_defer": v.induced_defer,
        "hosts_touched": step_keyed(v.hosts_touched.iter().map(|(n, hs)| {
            (*n, Value::Array(hs.iter().map(|h| match h {
                HostTouched::Host { host, directory } => json!({ "host": host, "directory": directory }),
                HostTouched::Unresolved(src) => json!({ "unresolved": src }),
            }).collect()))
        })),
        "deferred": v.deferred,
        "dispatch": { "source": v.dispatch.source, "host_contract_hash": v.dispatch.host_contract_hash },
        "may_conflicts": v.may_conflicts.iter().map(|m| json!({ "earlier": m.earlier, "later": m.later, "fact": m.fact, "refused": m.refused })).collect::<Vec<_>>(),
        "unresolved_bindings": v.unresolved_bindings,
        "diagnostics": v.diagnostics.iter().map(|d| json!({ "code": d.code.as_str(), "step": opt_u32(d.step), "message": d.message })).collect::<Vec<_>>(),
        "steps": v.steps.iter().map(|s| json!({
            "n": s.n,
            "op": s.op,
            "locus": s.locus,
            "undo": txt(&s.undo),
            "undo_locus": s.undo_locus,
            "refusal": s.refusal,
            "drift": txt(&s.drift),
            "gate": match &s.gate {
                Some(g) => json!({ "expr": g.expr, "step_digest": true, "window_s": dur(g.window), "wait_alone_at_s": dur(g.wait_alone_at) }),
                None => Value::Null,
            },
            "knell": match &s.knell {
                Some(k) => json!({ "guard": txt(&k.guard), "cost": k.cost, "ack": k.ack }),
                None => Value::Null,
            },
            "conditional": txt(&s.conditional),
            "footprint": s.footprint,
        })).collect::<Vec<_>>(),
    })
}
