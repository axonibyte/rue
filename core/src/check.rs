//! `check(site, requester, plan) -> Verdict`: docs/ROADMAP.md sections 5.3
//! to 5.7 and 5.11, assembled. A transcription of the prototype's checker;
//! where a rule looks odd it is a Phase 0 finding reproduced for byte parity
//! and named as such.
//!
//! The requester is an input: the requester exclusion (E0508) is a check-time
//! rule, and an offline check has no session to read it from (section 5.11).

use crate::algebra::{numbered, op_of, step_of};
use crate::artifact;
use crate::backstop::{
    coverage, heartbeat_violations, reach_violations, trigger_violations, TriggerViolation,
};
use crate::body::{Body, Prim};
use crate::closure;
use crate::diagnostics::Code;
use crate::explain::undo_line;
use crate::gates;
use crate::intent::{
    commit_not_last, commit_step, effective_wane, infer_intent, paths_without_commit, Intent,
};
use crate::interference::{
    anchor_duplicates, conflict, mayconflict, par_violations, Conflict, Fact,
};
use crate::model::*;
use crate::secrets;
use crate::util::nub;
use crate::verdict::*;

/// The host a step acts on, resolved where it can be: `Err` carries the name
/// of the output a bound host comes from.
fn step_host(p: &Plan, o: &Op) -> Result<Host, String> {
    match &o.locus {
        // Phase 0 finding: a :controller step resolves to the owner here (so
        // it is never deferred) while `hosts_touched` reports the controller.
        Locus::Controller | Locus::Target => Ok(p.owner.clone()),
        Locus::Host(HostRef::Static(h)) => Ok(h.clone()),
        Locus::Host(HostRef::Bound(b)) => Err(b.clone()),
    }
}

fn host_record<'a>(site: &'a Site, h: &str) -> Option<&'a HostRecord> {
    site.hosts.iter().find(|r| r.name == h)
}

/// The controller's reach, as the engine selects its executor: a hook the
/// site binds with `transport: :controller` first, `local()` otherwise.
///
/// One definition, read by both the checker here and the engine's executor
/// selection, because the defect E0608 exists for was exactly those two
/// disagreeing: the checker passed hook actions at the controller while the
/// engine could send the controller nowhere but `local()`.
pub const CONTROLLER_REACH: &[&str] = &["controller", "local"];

/// The transport the running engine would reach a host by, chosen as the
/// engine's `executor_index` chooses: the first of its `reach` that some
/// executor serves. `local()` is always one of them -- the daemon builds it
/// for the controller whatever the site says -- and every other executor
/// is one of the site's transports.
fn selected_transport<'a>(
    site: &Site,
    reach: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    reach
        .into_iter()
        .find(|t| *t == "local" || site.transports.iter().any(|s| s == t))
}

/// `local()` and `ssh()` perform `run` and the fact primitives; every other
/// transport is a hook's (docs/LANGUAGE.md: `hook(:x, transport: :t)` is
/// `t`), and only a hook performs a `hook(...)` action or answers a probe
/// by name.
fn is_hook_transport(t: &str) -> bool {
    t != "local" && t != "ssh"
}

/// Every probe the plan will observe, with the step it is observed at, in
/// `numbered`'s numbering -- exactly the engine's consumers: a guard wherever
/// one appears (preflight, assert, a `when`'s condition, an op's pre and
/// post, a knell's), a knell's cost when a human is asked to acknowledge it,
/// a deferred step's `handoff_done` (the reap pass observes it), and
/// `observe`. A `repeat over:` list is not among them: it may be a parameter
/// supplied at apply, which nothing observes. Nor is the cost of a knell
/// acknowledged `:none`, which nothing measures, since nobody is asked.
fn observed_probes(p: &Plan) -> Vec<(u32, String)> {
    fn go(it: &Item, next: &mut u32, out: &mut Vec<(u32, String)>) {
        match it {
            Item::Par { children } => children.iter().for_each(|c| go(c, next, out)),
            Item::Repeat { body, .. } => body.iter().for_each(|c| go(c, next, out)),
            Item::When {
                guard,
                then_,
                else_,
                ..
            } => {
                // A `when` is not a leaf; its condition is observed where its
                // first leaf would run.
                out.push((*next, guard.name.clone()));
                then_.iter().for_each(|c| go(c, next, out));
                else_.iter().for_each(|c| go(c, next, out));
            }
            leaf => {
                let n = *next;
                *next += 1;
                match leaf {
                    Item::Preflight { guards } => {
                        guards.iter().for_each(|g| out.push((n, g.name.clone())))
                    }
                    Item::Assert { guard, .. } => out.push((n, guard.name.clone())),
                    Item::Observe { probe, .. } => out.push((n, probe.clone())),
                    Item::Step(s) | Item::Knell(s) => {
                        let o = &s.op;
                        for g in o.pre.iter().chain(o.post.iter()) {
                            out.push((n, g.name.clone()));
                        }
                        if let Refusal::Knell { guard, cost, ack } = &o.refusal {
                            if let Some(g) = guard {
                                out.push((n, g.name.clone()));
                            }
                            if let (Cost::Probe(c), Ack::Gate(_)) = (cost, ack) {
                                out.push((n, c.clone()));
                            }
                        }
                        if let Some(h) = &o.handoff_done {
                            out.push((n, h.clone()));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let mut next = 1;
    let mut out = Vec::new();
    p.body.iter().for_each(|it| go(it, &mut next, &mut out));
    out
}

/// The undo body, when the undo is one.
fn undo_body(o: &Op) -> Option<&Body> {
    match &o.undo {
        Undo::Computed { body, .. } | Undo::Compensate { body, .. } => Some(body),
        Undo::Restore | Undo::NoUndo => None,
    }
}

/// Every body an op has, named as the surface names them.
fn bodies(o: &Op) -> Vec<(&'static str, &Body)> {
    let mut v = vec![("do", &o.do_)];
    if let Some(b) = undo_body(o) {
        v.push(("undo", b));
    }
    if let Some(b) = &o.suspend {
        v.push(("suspend", b));
    }
    if let Some(b) = &o.reestablish {
        v.push(("reestablish", b));
    }
    v
}

/// A step is deferred when its host is one the running engine cannot act
/// on: a static host none of the site's transports reach, or a host bound at
/// runtime (section 5.12).
pub fn deferred_steps(site: &Site, p: &Plan) -> Vec<u32> {
    let reachable = |h: &str| match host_record(site, h) {
        None => false,
        Some(r) => r.reach.iter().any(|t| site.transports.contains(t)),
    };
    numbered(&p.body)
        .into_iter()
        .filter_map(|(n, it)| {
            let o = op_of(it)?;
            let deferred = match step_host(p, o) {
                Err(_) => true,
                Ok(h) => h != p.owner && !reachable(&h),
            };
            deferred.then_some(n)
        })
        .collect()
}

/// Trigger text as the surface spells it.
pub fn trigger_pretty(t: &Trigger) -> String {
    match t {
        Trigger::After(d) => format!("after {}", d.render()),
        Trigger::UnlessConfirmed(d) => format!("unless confirmed within {}", d.render()),
        Trigger::UnlessHeartbeat {
            deadline,
            interval: None,
        } => format!("unless heartbeat within {}", deadline.render()),
        Trigger::UnlessHeartbeat {
            deadline,
            interval: Some(i),
        } => {
            format!(
                "unless heartbeat within {} every {}",
                deadline.render(),
                i.render()
            )
        }
    }
}

fn is_hold(r: &Refusal) -> bool {
    matches!(r, Refusal::Hold { .. })
}

fn is_knell(r: &Refusal) -> bool {
    matches!(r, Refusal::Knell { .. })
}

fn cost_text(c: &Cost) -> String {
    match c {
        Cost::Probe(p) => p.clone(),
        Cost::NoCost(_) => "none".to_string(),
    }
}

fn ack_text(a: &Ack) -> String {
    match a {
        Ack::Gate(g) => gates::render_gate(g),
        Ack::NoAck(_) => "none".to_string(),
    }
}

fn fact_text(f: &Fact) -> String {
    match &f.anchor {
        Some(a) => format!("{} (anchor {a})", f.shape),
        None => f.shape.clone(),
    }
}

fn kind_text(k: Kind) -> &'static str {
    match k {
        Kind::Owned => "owned",
        Kind::Region => "region",
        Kind::Modified => "modified",
        Kind::Derived => "derived",
        Kind::AppendOnly => "append_only",
        Kind::Held => "held",
    }
}

fn drift_name(o: &Op) -> String {
    match o.effective_drift() {
        Some(Drift::Clobber) => "clobber".to_string(),
        Some(Drift::Defer) => "defer".to_string(),
        None => "n/a".to_string(),
    }
}

/// Split step numbers into segments: before the first knell, and after each
/// knell up to the next.
pub fn split_segments(ns: &[u32], knells: &[u32]) -> Vec<Vec<u32>> {
    match knells.split_first() {
        None => vec![ns.to_vec()],
        Some((k, rest)) => {
            let mut out = vec![ns.iter().copied().take_while(|n| n < k).collect()];
            let after: Vec<u32> = ns.iter().copied().filter(|n| n > k).collect();
            out.extend(split_segments(&after, rest));
            out
        }
    }
}

pub fn check(site: &Site, requester: &str, p: &Plan) -> Verdict {
    let body = &p.body;
    let steps = numbered(body);
    let step_ops: Vec<(u32, &Op)> = steps
        .iter()
        .filter_map(|(n, it)| op_of(it).map(|o| (*n, o)))
        .collect();
    let intent = infer_intent(p);
    let deferred = deferred_steps(site, p);
    let first_knell = steps
        .iter()
        .find(|(_, it)| matches!(it, Item::Knell(_)))
        .map(|(n, _)| *n);
    let last_step = steps.len() as u32;
    let owner = p.owner.as_str();
    let auths = &site.authenticators;

    // A conflict whose fact is an anchor declared twice is reported as E0305,
    // the specific diagnosis, not also as E0301.
    let duplicate_anchors = anchor_duplicates(owner, body);
    let conflict_list: Vec<Conflict> = conflict(owner, body)
        .into_iter()
        .filter(|c| !duplicate_anchors.contains(c))
        .collect();

    // Reversibility, when the interference query finds no conflict
    // (otherwise E0301 and the verdict says step 0): with a knell, every
    // step before it; without one, the last mutating step.
    let reversible_through = if !conflict_list.is_empty() {
        0
    } else {
        match first_knell {
            Some(k) => k - 1,
            None => step_ops.iter().map(|(n, _)| *n).max().unwrap_or(0),
        }
    };

    // Holding: the first :hold step in each knell segment (Phase 0 finding:
    // section 5.5 says "at or before the first knell"; T2 holds after it).
    let knell_steps: Vec<u32> = {
        let mut v: Vec<u32> = steps
            .iter()
            .filter(|(_, it)| matches!(it, Item::Knell(_)))
            .map(|(n, _)| *n)
            .collect();
        v.sort_unstable();
        v
    };
    let all_ns: Vec<u32> = steps.iter().map(|(n, _)| *n).collect();
    let holds_at: Vec<u32> = split_segments(&all_ns, &knell_steps)
        .iter()
        .filter_map(|seg| {
            seg.iter()
                .find(|n| step_ops.iter().any(|(m, o)| m == *n && is_hold(&o.refusal)))
                .copied()
        })
        .collect();

    let ponr = first_knell.and_then(|n| {
        let o = step_ops.iter().find(|(m, _)| *m == n).map(|(_, o)| *o)?;
        match &o.refusal {
            Refusal::Knell { guard, cost, ack } => Some(PointOfNoReturn {
                step: n,
                guard: guard.as_ref().map(|g| g.name.clone()),
                cost: cost_text(cost),
                ack: ack_text(ack),
                gate: steps
                    .iter()
                    .find(|(m, _)| *m == n)
                    .and_then(|(_, it)| step_of(it))
                    .and_then(|s| s.gate.as_ref().map(gates::render_gate)),
            }),
            _ => None,
        }
    });
    let back_to = match first_knell {
        Some(n) if n < last_step => Some((last_step, n)),
        _ => None,
    };

    // Gates.
    let gate_verdict = p.gate.as_ref().map(|pg| {
        let r = gates::report(auths, &pg.expr);
        GateVerdict {
            satisfiable: r.satisfiable,
            min_distinct_humans: r.min_distinct_humans,
            zero_human_path: r.zero_human_path,
            window: pg.window,
            wait_alone_at: r.wait_alone_at,
        }
    });

    // A region step's damaged-marker fallback is conditional on no other
    // instance holding a region on the fact (sections 5.2, D-053).
    let conditional_steps: Vec<(u32, String)> = step_ops
        .iter()
        .filter(|(_, o)| o.effective_drift() == Some(Drift::Clobber))
        .flat_map(|(n, o)| {
            o.footprint
                .iter()
                .filter(|e| e.kind == Kind::Region)
                .map(move |e| (*n, format!("foreign region in {}", e.shape)))
        })
        .collect();

    // Backstop coverage.
    let cov = coverage(p);
    let backstop_verdict = match (&p.backstop, &cov) {
        (Some(b), Some(c)) => {
            let a = b.arm_before;
            let first_covered = c.installed_before;
            // Phase 0 finding, reproduced: with nothing covered, "armed
            // before" is reported rather than "armed after".
            let armed_before = if first_covered.is_none_or(|f| a <= f) {
                Some(a)
            } else {
                None
            };
            let armed_after = if armed_before.is_none() {
                Some(a - 1)
            } else {
                None
            };
            Some(BackstopVerdict {
                triggers: b.triggers.iter().map(trigger_pretty).collect(),
                covers: c.covered.clone(),
                installed_before: first_covered,
                armed_before,
                armed_after,
                late_arming_window: c.late_arming_window.clone(),
                scheduler: "cron".to_string(),
                granularity: Duration::new(60),
                self_enforced: true,
                drift_policy: step_ops
                    .iter()
                    .filter(|(n, _)| c.covered.contains(n))
                    .map(|(n, o)| (*n, drift_name(o)))
                    .collect(),
                snapshot_location: "target".to_string(),
                snapshot_cap: 1_048_576,
                conditional: conditional_steps.clone(),
            })
        }
        _ => None,
    };

    let controller_only: Vec<u32> = step_ops
        .iter()
        .filter(|(_, o)| o.undo_locus == UndoLocus::Controller)
        .map(|(n, _)| *n)
        .collect();
    // Section 8.2: every :hold step of a permanent plan, and every deferred one.
    let held_indefinitely: Vec<u32> = if intent == Some(Intent::Permanent) {
        let mut v: Vec<u32> = step_ops
            .iter()
            .filter(|(_, o)| is_hold(&o.refusal))
            .map(|(n, _)| *n)
            .collect();
        v.extend(deferred.iter().copied());
        let mut v = nub(&v);
        v.sort_unstable();
        v
    } else {
        Vec::new()
    };
    let induced_defer: Vec<u32> = if p.mode == Mode::Auto {
        step_ops
            .iter()
            .filter(|(_, o)| o.effective_drift() == Some(Drift::Defer))
            .map(|(n, _)| *n)
            .collect()
    } else {
        Vec::new()
    };
    // A :controller step's markers live on the controller; every other
    // step's on its host's instance directory, or on the controller when the
    // host has no filesystem (section 7.7).
    let hosts_touched: Vec<(u32, Vec<HostTouched>)> = step_ops
        .iter()
        .map(|(n, o)| {
            let touched = match &o.locus {
                Locus::Controller => HostTouched::Host {
                    host: "controller".into(),
                    directory: "controller".into(),
                },
                _ => match step_host(p, o) {
                    Err(b) => HostTouched::Unresolved(format!("bound from {b}")),
                    Ok(h) => {
                        let fs = host_record(site, &h).is_some_and(|r| r.filesystem);
                        HostTouched::Host {
                            host: h,
                            directory: if fs {
                                "target".into()
                            } else {
                                "controller".into()
                            },
                        }
                    }
                },
            };
            (*n, vec![touched])
        })
        .collect();

    // May-conflicts.
    let mays = mayconflict(owner, body);
    let may_conflict_verdicts: Vec<MayConflict> = mays
        .iter()
        .map(|c| MayConflict {
            earlier: c.earlier,
            later: c.later,
            fact: fact_text(&c.fact),
            refused: p.strictness == Strictness::Strict,
        })
        .collect();

    // Diagnostics, in generation order then stably by code.
    let d = |code: Code, n: Option<u32>, msg: String| Diagnostic {
        code,
        step: n,
        message: msg,
    };
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    let no_filesystem = |o: &Op| match step_host(p, o) {
        Ok(h) => host_record(site, &h).is_none_or(|r| !r.filesystem),
        Err(_) => false,
    };
    for (n, o) in &step_ops {
        let n = Some(*n);
        if (o.undo == Undo::NoUndo) != is_knell(&o.refusal) {
            diagnostics.push(d(
                Code::E0201,
                n,
                format!("op {}: undo and knell disagree", o.id),
            ));
        }
        if o.undo_locus == UndoLocus::Target {
            let unclosed = closure::unclosed_undo(o);
            if let Some(first) = unclosed.first() {
                diagnostics.push(d(
                    Code::E0202,
                    n,
                    format!(
                        "op {}: :target undo is not closed over target-local commands and facts: {}",
                        o.id,
                        closure::describe(first)
                    ),
                ));
            }
        }
        if o.undo_locus == UndoLocus::NoLocus && o.undo != Undo::NoUndo {
            diagnostics.push(d(
                Code::E0203,
                n,
                format!("op {}: undo_locus :none with an undo body", o.id),
            ));
        }
        if o.footprint.iter().any(|e| e.kind == Kind::Held)
            && (o.suspend.is_none() || o.reestablish.is_none())
        {
            diagnostics.push(d(
                Code::E0205,
                n,
                format!("op {}: held footprint without suspend/reestablish", o.id),
            ));
        }
        if matches!(&o.undo, Undo::Computed { undo_pre, .. } | Undo::Compensate { undo_pre, .. } if undo_pre.is_empty())
        {
            diagnostics.push(d(
                Code::E0207,
                n,
                format!(
                    "op {}: computed or compensating undo without undo_pre",
                    o.id
                ),
            ));
        }
        if !o.undo_idempotent {
            diagnostics.push(d(
                Code::E0208,
                n,
                format!("op {}: undo not provably idempotent", o.id),
            ));
        }
        if o.undo_locus == UndoLocus::Target && no_filesystem(o) {
            diagnostics.push(d(
                Code::E0407,
                n,
                format!(
                    "op {}: :target undo on a host with no run-capable executor",
                    o.id
                ),
            ));
        }
        if !o.reach.is_empty() && o.effective_drift() == Some(Drift::Defer) {
            diagnostics.push(d(
                Code::E0410,
                n,
                format!("op {}: reach op whose undo is drift: :defer", o.id),
            ));
        }

        // Secret placement (section 5.8), over every body the op has.
        let has_secret_output = o.outputs.iter().any(|out| out.secret);
        if has_secret_output && site.secrets_deliver_to.is_empty() {
            diagnostics.push(d(
                Code::E0606,
                n,
                format!(
                    "op {}: a secret output and the site declares no secrets deliver_to",
                    o.id
                ),
            ));
        }
        if has_secret_output {
            if let Some(i) = o
                .reestablish
                .as_ref()
                .and_then(|r| secrets::reruns(&o.do_, r))
            {
                diagnostics.push(d(
                    Code::E0206,
                    n,
                    format!(
                        "op {}: reestablish re-runs its do (prim {}); the secret output would be produced again",
                        o.id,
                        i + 1
                    ),
                ));
            }
        }
        if o.undo_locus == UndoLocus::Target {
            if let Some(f) = undo_body(o).and_then(secrets::anywhere) {
                diagnostics.push(d(
                    Code::E0210,
                    n,
                    format!(
                        "op {}: :target undo references secret {} ({}, prim {})",
                        o.id,
                        f.r.label(),
                        f.position,
                        f.index + 1
                    ),
                ));
            }
        }
        let preamble_host = match &o.locus {
            Locus::Controller => None,
            _ => step_host(p, o).ok().and_then(|h| {
                host_record(site, &h)
                    .filter(|r| !r.stdin_preamble)
                    .map(|r| r.name.clone())
            }),
        };
        for (name, body) in bodies(o) {
            if let Some(f) = secrets::in_run_string(body) {
                diagnostics.push(d(
                    Code::E0209,
                    n,
                    format!(
                        "op {}: secret {} interpolated into a run string ({}, prim {})",
                        o.id,
                        f.r.label(),
                        name,
                        f.index + 1
                    ),
                ));
            }
            if let (Some(h), Some(f)) = (&preamble_host, secrets::in_channel(body)) {
                diagnostics.push(d(
                    Code::E0211,
                    n,
                    format!(
                        "op {}: secret {} via {}: on {}, whose executor cannot honor the stdin preamble ({}, prim {})",
                        o.id,
                        f.r.label(),
                        f.position,
                        h,
                        name,
                        f.index + 1
                    ),
                ));
            }
        }
    }

    // E0608: an action the host's executor cannot perform. The engine sends
    // every step to one executor, chosen from the host's reach; `local()`
    // and `ssh()` refuse a `hook(...)` action and answer a probe only by
    // running its `run` body. A plan that asks either of them for anything
    // else checks clean and then fails at the step -- which is how T2 was
    // unrunnable for four phases while its verdict read as it should. A
    // step whose host nothing reaches is deferred, not refused, and is not
    // this code's: the engine never acts on it.
    let owner_reach: Vec<&str> = host_record(site, owner)
        .map(|r| r.reach.iter().map(String::as_str).collect())
        .unwrap_or_default();
    for (n, o) in &step_ops {
        let (host, reach): (String, Vec<&str>) = match &o.locus {
            Locus::Controller => ("the controller".to_string(), CONTROLLER_REACH.to_vec()),
            _ => match step_host(p, o).ok().and_then(|h| host_record(site, &h)) {
                Some(r) => (r.name.clone(), r.reach.iter().map(String::as_str).collect()),
                None => continue,
            },
        };
        let Some(t) = selected_transport(site, reach) else {
            continue;
        };
        if is_hook_transport(t) {
            continue;
        }
        for (name, body) in bodies(o) {
            if let Some(h) = body.iter().find_map(|prim| match prim {
                Prim::Hook(h) => Some(h),
                _ => None,
            }) {
                diagnostics.push(d(
                    Code::E0608,
                    Some(*n),
                    format!(
                        "op {}: hook(:{}) in its {name} body runs on {host}, reached by {t}(), which performs run and the fact primitives and never a hook's action",
                        o.id, h.name
                    ),
                ));
            }
        }
    }
    let mut seen_probes: Vec<String> = Vec::new();
    for (n, name) in observed_probes(p) {
        if seen_probes.contains(&name) {
            continue;
        }
        seen_probes.push(name.clone());
        let decl = p
            .probes
            .iter()
            .find(|d| d.name == name || d.produces.iter().any(|f| f == &name));
        let runs = decl.is_some_and(|d| d.body.iter().any(|prim| matches!(prim, Prim::Run(_))));
        if runs {
            continue;
        }
        let (host, reach): (&str, Vec<&str>) = match decl.map(|d| &d.locus) {
            Some(Locus::Controller) => ("the controller", CONTROLLER_REACH.to_vec()),
            _ => (owner, owner_reach.clone()),
        };
        let Some(t) = selected_transport(site, reach) else {
            continue;
        };
        if !is_hook_transport(t) {
            let why = if decl.is_some() {
                "is declared with no run body"
            } else {
                "has no declaration"
            };
            diagnostics.push(d(
                Code::E0608,
                Some(n),
                format!(
                    "probe {name} {why}, and is observed on {host}, reached by {t}(), which answers a probe only by running it"
                ),
            ));
        }
    }

    let has_force = steps
        .iter()
        .any(|(_, it)| step_of(it).is_some_and(|s| !s.force.is_empty()));
    if p.mode == Mode::Auto && has_force {
        diagnostics.push(d(
            Code::E0404,
            None,
            "mode: :auto plan contains force:".to_string(),
        ));
    }
    if p.backstop.is_some() && !site.scheduler_present.contains(&p.owner) {
        diagnostics.push(d(
            Code::E0403,
            None,
            format!("backstop scheduler not present on {}", p.owner),
        ));
    }
    // A :target backstop is an artifact on the owner, rendered in the
    // owner's declared language; no template for the pair is E0403
    // (section 4.5).
    if cov.as_ref().is_some_and(|c| !c.covered.is_empty()) {
        if let Some(h) = host_record(site, owner) {
            let lang = artifact::language_of(h);
            if !artifact::supported(&h.os, lang) {
                diagnostics.push(d(
                    Code::E0403,
                    None,
                    format!(
                        "backstop artifact: no {} template for os {} on {}",
                        lang.name(),
                        h.os,
                        owner
                    ),
                ));
            }
        }
    }

    for c in &conflict_list {
        diagnostics.push(d(
            Code::E0301,
            Some(c.later),
            format!(
                "steps {} and {} both write {} and step {}'s undo needs it",
                c.earlier,
                c.later,
                fact_text(&c.fact),
                c.earlier
            ),
        ));
    }
    if p.strictness == Strictness::Strict {
        for c in &mays {
            diagnostics.push(d(
                Code::E0302,
                Some(c.later),
                format!(
                    "may-conflict between steps {} and {} on {}",
                    c.earlier,
                    c.later,
                    fact_text(&c.fact)
                ),
            ));
        }
    }
    let (overlaps, reachy) = par_violations(owner, body);
    for (a, b) in overlaps {
        diagnostics.push(d(
            Code::E0303,
            Some(b),
            format!("par children at steps {a} and {b} are not umbra-disjoint"),
        ));
    }
    for n in reachy {
        diagnostics.push(d(Code::E0304, Some(n), "reach op inside par".to_string()));
    }
    for c in &duplicate_anchors {
        diagnostics.push(d(
            Code::E0305,
            Some(c.later),
            format!(
                "anchor declared twice on {} (steps {}, {})",
                fact_text(&c.fact),
                c.earlier,
                c.later
            ),
        ));
    }

    for n in reach_violations(p) {
        diagnostics.push(d(
            Code::E0401,
            Some(n),
            "reach op without a preceding armed :target backstop".to_string(),
        ));
    }
    for t in heartbeat_violations(p) {
        diagnostics.push(d(
            Code::E0405,
            None,
            format!(
                "heartbeat interval exceeds a third of its deadline: {}",
                trigger_pretty(&t)
            ),
        ));
    }
    if let Some(i) = intent {
        for tv in trigger_violations(i, p) {
            diagnostics.push(match tv {
                TriggerViolation::AfterNotWane => d(
                    Code::E0503,
                    None,
                    "temporary plan's backstop after: differs from its wane".to_string(),
                ),
                TriggerViolation::TemporaryConfirmed => d(
                    Code::E0503,
                    None,
                    "temporary plan's backstop is unless_confirmed without fires_by_construction"
                        .to_string(),
                ),
                TriggerViolation::PermanentAfter => d(
                    Code::E0504,
                    None,
                    "permanent plan's backstop expires on a timer".to_string(),
                ),
                TriggerViolation::NoConfirmOrCommitPath(k) => d(
                    Code::E0504,
                    None,
                    format!("{k} path(s) reach neither confirm() nor commit()"),
                ),
            });
        }
    }

    if intent.is_none() {
        diagnostics.push(d(
            Code::E0501,
            None,
            "intent undeterminable: both wane and commit(), or neither".to_string(),
        ));
    }
    if commit_not_last(body) {
        diagnostics.push(d(
            Code::E0502,
            None,
            "commit() is not the last item on its path".to_string(),
        ));
    }
    if intent == Some(Intent::Permanent) {
        let k = paths_without_commit(body);
        if k > 0 {
            diagnostics.push(d(
                Code::E0505,
                None,
                format!("{k} non-refusing path(s) never reach commit()"),
            ));
        }
    }

    // Waits and bounds (section 5.9, rules 2 and 3).
    let knell_waits = |r: &Refusal| match r {
        Refusal::Knell {
            ack: Ack::Gate(_), ..
        } => true,
        Refusal::Knell { guard: Some(g), .. } => g.value == Tri::Unknown,
        _ => false,
    };
    let waiting_steps: Vec<u32> = steps
        .iter()
        .filter(|(_, it)| match it {
            Item::Assert { .. } | Item::When { .. } => true,
            _ => step_of(it).is_some_and(|s| s.gate.is_some() || knell_waits(&s.op.refusal)),
        })
        .map(|(n, _)| *n)
        .collect();
    let can_wait = !waiting_steps.is_empty();
    let lapse_hold = |it: &Item| match it {
        Item::Assert { on_lapse, .. } | Item::When { on_lapse, .. } => *on_lapse == OnLapse::Hold,
        _ => step_of(it).is_some_and(|s| s.on_lapse == OnLapse::Hold),
    };
    let can_hold = !holds_at.is_empty() || steps.iter().any(|(_, it)| lapse_hold(it));
    let unbounded_wait = |n: u32| -> bool {
        let no_max = site.max_wait.is_none();
        match steps.iter().find(|(m, _)| *m == n).map(|(_, it)| *it) {
            Some(Item::Assert { window, .. }) | Some(Item::When { window, .. }) => {
                window.is_none() && no_max
            }
            Some(it) => step_of(it).is_some_and(|s| s.window.is_none() && no_max),
            None => false,
        }
    };
    // Phase 0 finding: Pending's bound is its approval window; a gated plan
    // with no window and no max_wait reserves umbras until cancelled.
    let pending_unbounded = || -> Vec<Diagnostic> {
        match &p.gate {
            Some(pg) if pg.window.is_none() && site.max_wait.is_none() => {
                vec![d(Code::E0506, None, "plan-entry gate has no window: and the site has no max_wait; Pending would be unbounded".to_string())]
            }
            _ => Vec::new(),
        }
    };
    match intent {
        Some(Intent::Temporary) => {
            if effective_wane(p).is_none() && (can_wait || can_hold || !deferred.is_empty()) {
                diagnostics.push(d(
                    Code::E0506,
                    None,
                    "temporary plan can reach Waiting, Held or Deferred without wane".to_string(),
                ));
            } else {
                diagnostics.extend(pending_unbounded());
            }
        }
        Some(Intent::Permanent) => {
            for n in &waiting_steps {
                if unbounded_wait(*n) {
                    diagnostics.push(d(
                        Code::E0506,
                        Some(*n),
                        "permanent plan's wait has neither window: nor a site max_wait".to_string(),
                    ));
                }
            }
            diagnostics.extend(pending_unbounded());
        }
        None => {}
    }

    // Gates.
    let gate_checks = |n: Option<u32>, g: &GateExpr, allow_zero: bool| -> Vec<Diagnostic> {
        let r = gates::report(auths, g);
        let mut v = Vec::new();
        let unknown = gates::unknown_authenticators(auths, g);
        if !unknown.is_empty() {
            v.push(d(
                Code::E0508,
                n,
                format!(
                    "gate names unknown authenticator(s): {}",
                    unknown.join(", ")
                ),
            ));
        }
        if !r.satisfiable {
            v.push(d(Code::E0508, n, "gate is unsatisfiable".to_string()));
        }
        if gates::counts_requester(auths, requester, g) {
            v.push(d(
                Code::E0508,
                n,
                format!("gate counts the requester {requester}"),
            ));
        }
        if r.zero_human_path && (!allow_zero || p.mode == Mode::Auto) {
            v.push(d(
                Code::E0509,
                n,
                "gate satisfiable with zero human authenticators and no allow_zero_human"
                    .to_string(),
            ));
        }
        if n.is_some() && p.mode == Mode::Auto && !r.zero_human_path {
            v.push(d(
                Code::E0507,
                n,
                "mode: :auto plan has a step gate needing a human".to_string(),
            ));
        }
        v
    };
    if let Some(pg) = &p.gate {
        diagnostics.extend(gate_checks(None, &pg.expr, pg.allow_zero_human));
    }
    for (n, it) in &steps {
        if let Some(g) = step_of(it).and_then(|s| s.gate.as_ref()) {
            diagnostics.extend(gate_checks(Some(*n), g, false));
        }
    }
    for (n, o) in &step_ops {
        if let Refusal::Knell {
            ack: Ack::Gate(g), ..
        } = &o.refusal
        {
            if p.mode == Mode::Auto {
                diagnostics.push(d(
                    Code::E0507,
                    Some(*n),
                    "mode: :auto plan has a knell whose ack: is not :none".to_string(),
                ));
            }
            let unknown = gates::unknown_authenticators(auths, g);
            if !unknown.is_empty() {
                diagnostics.push(d(
                    Code::E0508,
                    Some(*n),
                    format!("ack names unknown authenticator(s): {}", unknown.join(", ")),
                ));
            }
            if !gates::report(auths, g).satisfiable {
                diagnostics.push(d(Code::E0508, Some(*n), "ack is unsatisfiable".to_string()));
            }
        }
    }
    // Stable: same-code diagnostics keep generation order.
    diagnostics.sort_by_key(|x| x.code);

    let step_verdicts: Vec<StepVerdict> = steps
        .iter()
        .map(|(n, it)| match op_of(it) {
            Some(o) => StepVerdict {
                n: *n,
                op: o.id.clone(),
                locus: match &o.locus {
                    Locus::Controller => "controller".to_string(),
                    Locus::Target => "target".to_string(),
                    Locus::Host(HostRef::Static(h)) => format!("host({h})"),
                    Locus::Host(HostRef::Bound(b)) => format!("host bound from {b}"),
                },
                undo: match o.undo {
                    Undo::NoUndo => None,
                    _ => Some(undo_line(o)),
                },
                undo_locus: match o.undo_locus {
                    UndoLocus::Target => "target",
                    UndoLocus::Controller => "controller",
                    UndoLocus::NoLocus => "none",
                }
                .to_string(),
                refusal: match o.refusal {
                    Refusal::Revert => "revert",
                    Refusal::Hold { .. } => "hold",
                    Refusal::Knell { .. } => "knell",
                }
                .to_string(),
                drift: match o.effective_drift() {
                    Some(Drift::Clobber) => Some("clobber".to_string()),
                    Some(Drift::Defer) => Some("defer".to_string()),
                    None => None,
                },
                gate: step_of(it).and_then(|s| {
                    s.gate.as_ref().map(|g| StepGateVerdict {
                        expr: gates::render_gate(g),
                        window: s.window,
                        wait_alone_at: gates::report(auths, g).wait_alone_at,
                    })
                }),
                knell: match &o.refusal {
                    Refusal::Knell { guard, cost, ack } => Some(KnellVerdict {
                        guard: guard.as_ref().map(|g| g.name.clone()),
                        cost: cost_text(cost),
                        ack: ack_text(ack),
                    }),
                    _ => None,
                },
                conditional: conditional_steps
                    .iter()
                    .find(|(m, _)| m == n)
                    .map(|(_, c)| c.clone()),
                footprint: o
                    .footprint
                    .iter()
                    .map(|e| {
                        format!(
                            "{}: {}{}",
                            kind_text(e.kind),
                            e.shape,
                            e.anchor
                                .as_ref()
                                .map(|a| format!(" anchor {a}"))
                                .unwrap_or_default()
                        )
                    })
                    .collect(),
            },
            None => StepVerdict {
                n: *n,
                op: match it {
                    Item::Confirm => "confirm()".to_string(),
                    Item::Commit => "commit()".to_string(),
                    Item::Preflight { .. } => "preflight".to_string(),
                    Item::Observe { probe, .. } => format!("observe {probe}"),
                    Item::Assert { guard, .. } => format!("assert {}", guard.name),
                    Item::Slot { name } => format!("slot :{name}"),
                    _ => "item".to_string(),
                },
                locus: "controller".to_string(),
                undo: None,
                undo_locus: "none".to_string(),
                refusal: "n/a".to_string(),
                drift: None,
                gate: None,
                knell: None,
                conditional: None,
                footprint: Vec::new(),
            },
        })
        .collect();

    Verdict {
        plan: p.id.clone(),
        host: p.owner.clone(),
        status: if diagnostics.is_empty() {
            Status::Ok
        } else {
            Status::Refused
        },
        intent,
        rehearsal: false,
        mode: match p.mode {
            Mode::Manual => "manual".to_string(),
            Mode::Auto => "auto".to_string(),
        },
        commit_step: if intent == Some(Intent::Permanent) {
            commit_step(body)
        } else {
            None
        },
        fires_by_construction: p.fires_by_construction,
        wane: effective_wane(p),
        reversible_through,
        holds_at,
        point_of_no_return: ponr,
        reversible_back_to: back_to,
        gate: gate_verdict,
        backstop: backstop_verdict,
        controller_only_undos: controller_only,
        held_indefinitely,
        induced_defer,
        hosts_touched,
        deferred,
        dispatch: Dispatch {
            source: "inventory".to_string(),
            host_contract_hash: "-".to_string(),
        },
        may_conflicts: may_conflict_verdicts,
        unresolved_bindings: nub(&step_ops
            .iter()
            .filter_map(|(_, o)| match &o.locus {
                Locus::Host(HostRef::Bound(b)) => Some(b.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()),
        diagnostics,
        steps: step_verdicts,
    }
}
