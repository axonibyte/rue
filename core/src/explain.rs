//! `explain`, docs/ROADMAP.md Appendix B: one line per numbered step.
//!
//! ```text
//!  N. <op>[(<args>)]   locus=<L>   refusal=<R>   drift=<D>   undo=<one-line undo or "NO UNDO — knell, cost <C>">   undo_locus=<UL>   [gate=<G>]   [ack=<A>]   [deferred → <handoff cmd>]
//! ```
//!
//! Secrets render as `<secret:label>`; a `region` op with `drift: :clobber`
//! prints its damaged-marker cost.

use crate::algebra::numbered;
use crate::body::{Body, Part, Prim, Ref, Value};
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
        _ => undo_line(o),
    }
}

/// The one-line undo of an op, derived from its undo: per footprint entry for
/// `Restore`, per primitive for a computed or compensating body. What
/// `explain` prints, `apply` shows and the `Applying` journal entry carries;
/// it can claim no more than the body does.
pub fn undo_line(o: &Op) -> String {
    match &o.undo {
        Undo::NoUndo => String::new(),
        Undo::Restore => {
            let parts: Vec<String> = o
                .footprint
                .iter()
                .filter_map(|e| match e.kind {
                    Kind::Owned => Some(format!("remove {}", e.shape)),
                    Kind::Region => Some(match &e.anchor {
                        Some(a) => format!("strip anchor {a} from {}", e.shape),
                        None => format!("strip the region from {}", e.shape),
                    }),
                    Kind::Modified => Some(format!("restore {} from snapshot", e.shape)),
                    Kind::Held => Some(format!("release {}", e.shape)),
                    Kind::Derived | Kind::AppendOnly => None,
                })
                .collect();
            if parts.is_empty() {
                "nothing to restore".to_string()
            } else {
                parts.join("; ")
            }
        }
        Undo::Computed { body, .. } => body_line(body),
        Undo::Compensate { body, .. } => {
            let record = if o.footprint.iter().any(|e| e.kind == Kind::AppendOnly) {
                " (undone by record, not erasure)"
            } else {
                ""
            };
            format!("compensate: {}{record}", body_line(body))
        }
    }
}

/// A body as one line, primitives joined by `; `.
pub fn body_line(body: &Body) -> String {
    body.iter().map(prim_line).collect::<Vec<_>>().join("; ")
}

fn kwargs(args: impl Iterator<Item = (String, String)>) -> String {
    args.map(|(n, v)| format!("{n}: {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn prim_line(p: &Prim) -> String {
    match p {
        Prim::Run(r) => template_text(&r.cmd),
        Prim::Write(w) => format!("write {}", w.fact.shape),
        Prim::Remove(r) => format!("remove {}", r.fact.shape),
        Prim::Append(a) => format!("append to {}", a.fact.shape),
        Prim::RegionSet(r) => match &r.fact.anchor {
            Some(a) => format!("set anchor {a} in {}", r.fact.shape),
            None => format!("set the region in {}", r.fact.shape),
        },
        Prim::RegionClear(r) => match &r.fact.anchor {
            Some(a) => format!("clear anchor {a} in {}", r.fact.shape),
            None => format!("clear the region in {}", r.fact.shape),
        },
        Prim::Stage(s) => format!("stage {}", s.name),
        Prim::Hook(h) => format!(
            "hook :{}({})",
            h.name,
            kwargs(
                h.args
                    .iter()
                    .map(|a| (a.name.clone(), value_text(&a.value)))
            )
        ),
        Prim::Install(i) => format!("install :{}", i.name),
        Prim::Release(r) => format!("release :{}", r.name),
        Prim::Call(c) => format!(
            "{}({})",
            c.prim,
            kwargs(
                c.args
                    .iter()
                    .map(|a| (a.name.clone(), value_text(&a.value)))
            )
        ),
    }
}

/// A value as `explain` prints it: literals verbatim, references as
/// `#{name}`, secrets as `<secret:name>`.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Lit(s) => s.clone(),
        Value::Ref(r) => ref_text(r),
        Value::Template(parts) => template_text(parts),
    }
}

fn template_text(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Lit(s) => s.clone(),
            Part::Ref(r) => ref_text(r),
        })
        .collect()
}

fn ref_text(r: &Ref) -> String {
    if r.is_secret() {
        format!("<secret:{}>", r.label())
    } else {
        format!("#{{{}}}", r.label())
    }
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

// ---------------------------------------------------------------------------
// The same listing as one self-contained page (Phase 5; docs/issues/0007)

/// `explain --html`: the listing above, and the verdict's prose where there
/// is one, as a single page that depends on nothing.
///
/// Self-contained is the whole point: an operator pastes it into a change
/// record, mails it to an approver, or keeps it beside an incident, and it
/// has to say the same thing years later on a machine with no network.
/// There is no script, no font, no stylesheet and no image -- what is not
/// in the file cannot be fetched, and what cannot be fetched cannot change
/// what the page says after it was read.
///
/// Every value goes through `escape`, because a plan's text is a tenant's
/// to write: an op named `<script>` renders as characters, not as markup.
pub fn explain_html(p: &Plan, deferred_steps: &[u32], verdict_prose: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(&format!(
        "<title>rue explain: {} on {}</title>\n",
        escape(&p.id),
        escape(&p.owner)
    ));
    out.push_str(STYLE);
    out.push_str("</head>\n<body>\n");
    out.push_str(&format!(
        "<h1>{} <span class=\"host\">on {}</span></h1>\n",
        escape(&p.id),
        escape(&p.owner)
    ));
    if let Some(text) = verdict_prose {
        out.push_str("<section class=\"verdict\">\n");
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push_str(&format!("<p>{}</p>\n", escape(line)));
        }
        out.push_str("</section>\n");
    }
    out.push_str("<table>\n<thead>\n<tr><th>#</th><th>step</th><th>locus</th><th>refusal</th><th>drift</th><th>undo</th><th>undo locus</th><th>notes</th></tr>\n</thead>\n<tbody>\n");
    for (n, it) in numbered(&p.body) {
        out.push_str(&row(n, it, deferred_steps));
    }
    out.push_str("</tbody>\n</table>\n");
    out.push_str(
        "<p class=\"foot\">Rendered by <code>rue explain --html</code>. \
         Every mutating step shows the undo it will run before it runs; a step \
         with <code>NO UNDO</code> is a knell and names its cost.</p>\n",
    );
    out.push_str("</body>\n</html>\n");
    out
}

/// Enough style to read a plan on a phone at two in the morning, and not a
/// byte that has to be fetched.
const STYLE: &str = "<style>\n\
:root { color-scheme: light dark; }\n\
body { font: 16px/1.5 system-ui, sans-serif; margin: 2rem auto; max-width: 60rem; padding: 0 1rem; }\n\
h1 { font-size: 1.5rem; margin-bottom: 0.25rem; }\n\
h1 .host { font-weight: normal; opacity: 0.7; }\n\
.verdict { border-left: 4px solid currentColor; padding-left: 1rem; opacity: 0.9; }\n\
table { border-collapse: collapse; width: 100%; margin-top: 1.5rem; }\n\
th, td { border-bottom: 1px solid rgba(128,128,128,0.4); padding: 0.4rem 0.5rem; text-align: left; vertical-align: top; }\n\
th { font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.04em; opacity: 0.7; }\n\
td.n { text-align: right; font-variant-numeric: tabular-nums; opacity: 0.7; }\n\
td.step { font-weight: 600; }\n\
code, td.undo { font-family: ui-monospace, monospace; font-size: 0.9em; }\n\
tr.knell td.undo { font-weight: 600; }\n\
td.notes { font-size: 0.9em; opacity: 0.85; }\n\
.foot { margin-top: 2rem; font-size: 0.85em; opacity: 0.7; }\n\
@media print { body { max-width: none; } }\n\
</style>\n";

fn cell(class: &str, text: &str) -> String {
    format!("<td class=\"{class}\">{}</td>", escape(text))
}

fn row(n: u32, it: &Item, deferred_steps: &[u32]) -> String {
    let (step, s) = match it {
        Item::Step(s) | Item::Knell(s) => (s.op.id.clone(), Some(s)),
        other => (body(n, other, deferred_steps), None),
    };
    let Some(s) = s else {
        // An item that is not a step is its own sentence, and spans the
        // columns a step fills rather than pretending to have them.
        return format!(
            "<tr class=\"item\"><td class=\"n\">{n}</td><td class=\"step\" colspan=\"7\">{}</td></tr>\n",
            escape(&step)
        );
    };
    let o = &s.op;
    let args = if s.args.is_empty() {
        String::new()
    } else {
        format!("({})", s.args.join(", "))
    };
    let mut notes: Vec<String> = Vec::new();
    if let Some(g) = &s.gate {
        notes.push(format!("gate={}", render_gate(g)));
    }
    match &o.refusal {
        Refusal::Knell {
            ack: Ack::Gate(g), ..
        } => notes.push(format!("ack={}", render_gate(g))),
        Refusal::Knell {
            ack: Ack::NoAck(reason),
            ..
        } => notes.push(format!("ack=none ({reason})")),
        _ => {}
    }
    if o.footprint.iter().any(|e| e.kind == Kind::Region)
        && o.effective_drift() == Some(Drift::Clobber)
    {
        notes.push("damaged-marker cost: the whole fact is restored from the do-time snapshot and a stranger's edits outside the region are lost, unless another instance holds a region on it".to_string());
    }
    if deferred_steps.contains(&n) {
        notes.push(format!(
            "deferred \u{2192} {}",
            o.handoff_done
                .as_ref()
                .map(|h| format!("handoff_done: {h}"))
                .unwrap_or_else(|| "(handoff command printed at apply)".to_string())
        ));
    }
    let knell = matches!(o.refusal, Refusal::Knell { .. });
    format!(
        "<tr{}><td class=\"n\">{n}</td>{}{}{}{}{}{}{}</tr>\n",
        if knell { " class=\"knell\"" } else { "" },
        cell("step", &format!("{}{args}", o.id)),
        cell("locus", &locus_text(&o.locus)),
        cell("refusal", &refusal_text(&o.refusal)),
        cell("drift", drift_text(o)),
        cell("undo", &undo_text(o)),
        cell("undolocus", undo_locus_text(o.undo_locus)),
        cell("notes", &notes.join("; ")),
    )
}

/// The five characters that are markup, and nothing else: a page that
/// escaped more would print a plan's own text back wrongly.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
