//! `explain`, docs/ROADMAP.md Appendix B: one line per numbered step.
//!
//! ```text
//!  N. <op>[(<args>)]   locus=<L>   refusal=<R>   drift=<D>   undo=<one-line undo or "NO UNDO — knell, cost <C>">   undo_locus=<UL>   [gate=<G>]   [ack=<A>]   [deferred → <handoff cmd>]
//! ```
//!
//! Secrets render as `<secret:label>`; a `region` op with `drift: :clobber`
//! prints its damaged-marker cost.

use crate::algebra::numbered;
use crate::gates::render_gate;
use crate::model::*;

pub fn explain(p: &Plan, deferred_steps: &[u32]) -> String {
    let mut out = String::new();
    for (n, it) in numbered(&p.body) {
        out.push_str(&format!("{n:>2}. {}\n", body(n, it, deferred_steps)));
    }
    out
}

fn body(n: u32, it: &Item, deferred_steps: &[u32]) -> String {
    match it {
        Item::Step(s) | Item::Knell(s) => step_line(n, s, deferred_steps),
        Item::Confirm => "confirm()   disarms the unless_confirmed backstop".to_string(),
        Item::Commit => {
            "commit()   ends the plan: undo discarded, umbras released, backstops disarmed"
                .to_string()
        }
        Item::Preflight { guards } => format!(
            "preflight   guards: {}   (measured before any mutation and again at point of use)",
            guards
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Item::Observe { probe, alias } => {
            format!("observe {probe} as {alias}   no footprint, no undo")
        }
        Item::Assert { guard, .. } => {
            format!("assert {}   :no refuses; :unknown waits", guard.name)
        }
        Item::Slot { name } => format!("slot :{name}"),
        // Containers never appear among the numbered leaves; named for totality.
        Item::Par { .. } => "par".to_string(),
        Item::Repeat { .. } => "repeat".to_string(),
        Item::When { guard, .. } => format!("when {}", guard.name),
    }
}

fn step_line(n: u32, s: &StepI, deferred_steps: &[u32]) -> String {
    let o = &s.op;
    let args = if s.args.is_empty() {
        String::new()
    } else {
        format!("({})", s.args.join(", "))
    };
    let mut parts = vec![
        format!("{}{args}", o.id),
        format!("locus={}", locus_text(&o.locus)),
        format!("refusal={}", refusal_text(&o.refusal)),
        format!("drift={}", drift_text(o)),
        format!("undo={}", undo_text(o)),
        format!("undo_locus={}", undo_locus_text(o.undo_locus)),
    ];
    if let Some(g) = &s.gate {
        parts.push(format!("gate={}", render_gate(g)));
    }
    match &o.refusal {
        Refusal::Knell {
            ack: Ack::Gate(g), ..
        } => parts.push(format!("ack={}", render_gate(g))),
        Refusal::Knell {
            ack: Ack::NoAck(reason),
            ..
        } => parts.push(format!("ack=none ({reason})")),
        _ => {}
    }
    if o.footprint.iter().any(|e| e.kind == Kind::Region)
        && o.effective_drift() == Some(Drift::Clobber)
    {
        parts.push("damaged-marker cost: the whole fact is restored from the do-time snapshot and a stranger's edits outside the region are lost, unless another instance holds a region on it".to_string());
    }
    if deferred_steps.contains(&n) {
        parts.push(format!(
            "deferred \u{2192} {}",
            o.handoff_done
                .as_ref()
                .map(|h| format!("handoff_done: {h}"))
                .unwrap_or_else(|| "(handoff command printed at apply)".to_string())
        ));
    }
    parts.join("   ")
}

fn locus_text(l: &Locus) -> String {
    match l {
        Locus::Controller => "controller".to_string(),
        Locus::Target => "target".to_string(),
        Locus::Host(HostRef::Static(h)) => format!("host({h})"),
        Locus::Host(HostRef::Bound(b)) => format!("host({b}) bound at runtime"),
    }
}

fn refusal_text(r: &Refusal) -> String {
    match r {
        Refusal::Revert => "revert".to_string(),
        Refusal::Hold { via: None } => "hold".to_string(),
        Refusal::Hold { via: Some(v) } => format!("hold via {v}"),
        Refusal::Knell { .. } => "knell".to_string(),
    }
}

fn drift_text(o: &Op) -> &'static str {
    match o.effective_drift() {
        Some(Drift::Clobber) => "clobber",
        Some(Drift::Defer) => "defer",
        None => "n/a",
    }
}

fn undo_text(o: &Op) -> String {
    match &o.undo {
        Undo::NoUndo => format!("NO UNDO \u{2014} knell, cost {}", cost_text(&o.refusal)),
        _ => redact(&o.undo_one_line, o),
    }
}

/// Secret outputs are redacted in the undo line. The prototype folds from
/// the right, so the last declared output is replaced first; the order
/// matters only when one output's name contains another's.
fn redact(t: &str, o: &Op) -> String {
    let mut acc = t.to_string();
    for out in o.outputs.iter().rev() {
        if out.secret && !out.name.is_empty() {
            acc = acc.replace(&out.name, &format!("<secret:{}>", out.name));
        }
    }
    acc
}

fn cost_text(r: &Refusal) -> String {
    match r {
        Refusal::Knell {
            cost: Cost::Probe(p),
            ..
        } => p.clone(),
        Refusal::Knell {
            cost: Cost::NoCost(reason),
            ..
        } => format!("none ({reason})"),
        _ => "none".to_string(),
    }
}

fn undo_locus_text(u: UndoLocus) -> &'static str {
    match u {
        UndoLocus::Target => "target",
        UndoLocus::Controller => "controller",
        UndoLocus::NoLocus => "none",
    }
}
