//! The verdict prose, docs/ROADMAP.md Appendix A: one clause per field group,
//! in this order: intent, reversibility, hold, point of no return, gate, step
//! gates, backstop, conditionals, controller-only undos, hosts touched,
//! deferred, dispatch, may-conflicts, unresolved bindings. A refused verdict
//! leads with its diagnostics instead.

use crate::intent::Intent;
use crate::verdict::*;

pub fn prose(v: &Verdict) -> String {
    let head = format!("{} on {}: ", v.plan, v.host);
    match v.status {
        Status::Refused => {
            let diags: Vec<String> = v
                .diagnostics
                .iter()
                .map(|d| {
                    format!(
                        "{}{}: {}",
                        d.code,
                        d.step.map(|n| format!(" at step {n}")).unwrap_or_default(),
                        d.message
                    )
                })
                .collect();
            format!("{head}refused; {}.\n", diags.join("; "))
        }
        Status::Ok => format!("{head}{}.\n", clauses(v).join("; ")),
    }
}

fn steps(ns: &[u32]) -> String {
    ns.iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn range(ns: &[u32]) -> String {
    match ns {
        [] => "no steps".to_string(),
        [n] => format!("step {n}"),
        _ => format!(
            "steps {}\u{2013}{}",
            ns.iter().min().unwrap(),
            ns.iter().max().unwrap()
        ),
    }
}

fn clauses(v: &Verdict) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    let induced = match v.induced_defer.as_slice() {
        [] => String::new(),
        ns => format!(", revert can be induced to defer at step {}", steps(ns)),
    };
    out.push(match v.intent {
        Some(Intent::Temporary) => format!(
            "temporary; reverts at wane {}{induced}",
            v.wane
                .map(|d| d.render())
                .unwrap_or_else(|| "?".to_string())
        ),
        Some(Intent::Permanent) => {
            let held = match v.held_indefinitely.as_slice() {
                [] => String::new(),
                ns => format!(
                    ", held indefinitely at step {} until an operator acts",
                    steps(ns)
                ),
            };
            format!(
                "permanent; commits at step {}{held}{}{induced}",
                v.commit_step
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string()),
                if v.fires_by_construction {
                    ", undo fires by construction"
                } else {
                    ""
                }
            )
        }
        None => "intent undetermined".to_string(),
    });

    let total = v.steps.len() as u32;
    out.push(if v.reversible_through == 0 {
        "not reversible past step 0".to_string()
    } else if v.reversible_through >= total && v.point_of_no_return.is_none() {
        format!("fully reversible ({total} steps)")
    } else {
        format!("reversible through step {}", v.reversible_through)
    });

    // Phase 0 finding: Appendix A has one hold clause, "(human required)";
    // section 8.2 says a hold under mode: :auto holds "until resume, recant
    // or commit", with no human in the loop, and the two cannot both be true.
    for n in &v.holds_at {
        out.push(if v.mode == "auto" {
            format!("step {n} holds on refusal (until resume, recant or commit)")
        } else {
            format!("step {n} holds on refusal (human required)")
        });
    }

    if let Some(p) = &v.point_of_no_return {
        out.push(format!(
            "step {} is a point of no return{}, cost {}, acknowledged by {}{}",
            p.step,
            p.guard
                .as_ref()
                .map(|g| format!(", guard {g}"))
                .unwrap_or_default(),
            p.cost,
            p.ack,
            v.reversible_back_to
                .map(|(f, t)| format!("; step {f} reversible back to step {t}"))
                .unwrap_or_default()
        ));
    }

    if let Some(g) = &v.gate {
        out.push(if !g.satisfiable {
            "gate unsatisfiable".to_string()
        } else if g.zero_human_path {
            "gate satisfiable with no human (allowed)".to_string()
        } else {
            format!(
                "gate satisfiable; minimum {} distinct humans",
                g.min_distinct_humans
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string())
            )
        });
    }

    for s in &v.steps {
        if let Some(g) = &s.gate {
            out.push(format!(
                "step {} gated by {}{}",
                s.n,
                g.expr,
                g.wait_alone_at
                    .map(|d| format!(", satisfiable by wait alone at +{}", d.render()))
                    .unwrap_or_default()
            ));
        }
    }

    if let Some(b) = &v.backstop {
        let mut c = format!(
            "expiry backstop ({}) covers {} on the target",
            b.triggers.join(", "),
            range(&b.covers)
        );
        if let Some(n) = b.installed_before {
            c.push_str(&format!(", installed before step {n}"));
        }
        if let Some(n) = b.armed_before {
            c.push_str(&format!(", armed before step {n}"));
        }
        if let Some(n) = b.armed_after {
            c.push_str(&format!(", armed after step {n}"));
        }
        if !b.late_arming_window.is_empty() {
            c.push_str(&format!(
                ", engine-only for {} until armed",
                range(&b.late_arming_window)
            ));
        }
        c.push_str(&format!(
            ", fires within ~{} after the deadline",
            b.granularity.render()
        ));
        if b.self_enforced {
            c.push_str(&format!(", self-enforced on {}", v.host));
        }
        if !b.drift_policy.is_empty() {
            c.push_str(&format!(
                ", drift: {}",
                b.drift_policy
                    .iter()
                    .map(|(n, p)| format!("{n} {p}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        c.push_str(&format!(", snapshots on target (cap {})", b.snapshot_cap));
        out.push(c);
        for (n, cond) in &b.conditional {
            out.push(format!(
                "step {n} reverts unaided unless {cond}; then deferred"
            ));
        }
    }

    match v.controller_only_undos.as_slice() {
        [] => {}
        [n] => out.push(format!("step {n} reverts only while the engine lives")),
        ns => out.push(format!(
            "steps {} revert only while the engine lives",
            steps(ns)
        )),
    }

    for (n, hs) in &v.hosts_touched {
        let skip = match hs.as_slice() {
            [HostTouched::Host { host, directory }] => {
                (host == &v.host && directory == "target") || host == "controller"
            }
            _ => false,
        };
        if skip {
            continue;
        }
        let texts: Vec<String> = hs
            .iter()
            .map(|h| match h {
                HostTouched::Host { host, directory } if directory == "controller" => {
                    format!("{host} (no instance directory; markers on controller)")
                }
                HostTouched::Host { host, .. } => host.clone(),
                HostTouched::Unresolved(_) => "a host bound at runtime".to_string(),
            })
            .collect();
        out.push(format!("step {n} touches {}", texts.join(", ")));
    }

    if !v.deferred.is_empty() {
        out.push(format!(
            "step {} deferred (handoff printed)",
            steps(&v.deferred)
        ));
    }

    out.push(format!("clause dispatch from {}", v.dispatch.source));

    for m in &v.may_conflicts {
        out.push(format!(
            "may-conflict between steps {} and {} on {}{}",
            m.earlier,
            m.later,
            m.fact,
            if m.refused {
                " (strict: refused)"
            } else {
                " (warn)"
            }
        ));
    }

    if !v.unresolved_bindings.is_empty() {
        out.push(format!(
            "unresolved binding(s): {}",
            v.unresolved_bindings.join(", ")
        ));
    }
    out
}
