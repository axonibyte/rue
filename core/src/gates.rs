//! Gates, docs/ROADMAP.md section 5.11: satisfiability, the minimum number of
//! distinct human authenticators on any satisfying path, the zero-human path,
//! the requester exclusion, and the earliest instant a gate is satisfiable
//! by wait alone.
//!
//! A gate is a weighted threshold over factors; a factor is an
//! authenticator, any human, a nested group, or a wait. Factor subsets are
//! enumerated, which is exact and small enough for every tenant.

use crate::model::{Authenticator, Duration, Factor, GateExpr};
use crate::util::nub;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateReport {
    pub satisfiable: bool,
    /// Over satisfying paths; `None` if unsatisfiable.
    pub min_distinct_humans: Option<usize>,
    pub zero_human_path: bool,
    /// Earliest instant satisfiable by waits only.
    pub wait_alone_at: Option<Duration>,
}

/// One way of satisfying a gate: which human authenticators it uses and the
/// longest wait it needs. Non-human authenticators contribute weight and no
/// human.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Path {
    humans: Vec<String>,
    wait: Option<Duration>,
}

fn factor_weight(f: &Factor) -> u32 {
    match f {
        Factor::Auth { weight, .. }
        | Factor::Humans { weight }
        | Factor::Group { weight, .. }
        | Factor::Wait { weight, .. } => *weight,
    }
}

fn factor_paths(auths: &[Authenticator], f: &Factor) -> Vec<Path> {
    match f {
        Factor::Auth { id, .. } => match auths.iter().find(|a| a.id == *id) {
            Some(a) => vec![Path {
                humans: if a.human {
                    vec![id.clone()]
                } else {
                    Vec::new()
                },
                wait: None,
            }],
            // An unknown authenticator satisfies nothing.
            None => Vec::new(),
        },
        Factor::Humans { .. } => auths
            .iter()
            .filter(|a| a.human)
            .map(|a| Path {
                humans: vec![a.id.clone()],
                wait: None,
            })
            .collect(),
        Factor::Group { expr, .. } => satisfying_paths(auths, expr),
        Factor::Wait { duration, .. } => vec![Path {
            humans: Vec::new(),
            wait: Some(*duration),
        }],
    }
}

fn merge(p: &Path, q: &Path) -> Path {
    let mut humans = p.humans.clone();
    humans.extend(q.humans.iter().cloned());
    Path {
        humans: nub(&humans),
        wait: match (p.wait, q.wait) {
            (None, x) | (x, None) => x,
            (Some(x), Some(y)) => Some(x.max(y)),
        },
    }
}

/// Every satisfying path of a gate against the binding's authenticators.
fn satisfying_paths(auths: &[Authenticator], g: &GateExpr) -> Vec<Path> {
    match g {
        GateExpr::Single(f) => factor_paths(auths, f),
        GateExpr::Thresh { n, factors } => {
            let n = *n;
            let mut out = Vec::new();
            // Every subset of factors, by bitmask; the order of subsets does
            // not reach the report (minimum, any, earliest).
            for mask in 0u64..(1u64 << factors.len()) {
                let chosen: Vec<&Factor> = factors
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| mask & (1 << i) != 0)
                    .map(|(_, f)| f)
                    .collect();
                let weights: Vec<u32> = chosen.iter().map(|f| factor_weight(f)).collect();
                let total: u32 = weights.iter().sum();
                if total < n {
                    continue;
                }
                // Skip supersets that add nothing: every factor is needed for the sum.
                let minimal = weights.iter().all(|w| total - w < n) || weights.is_empty();
                if !minimal {
                    continue;
                }
                // A path per choice of one satisfying path for each chosen factor.
                let mut acc = vec![Path {
                    humans: Vec::new(),
                    wait: None,
                }];
                for f in chosen.iter().rev() {
                    let fps = factor_paths(auths, f);
                    let mut next = Vec::new();
                    for p in &fps {
                        for q in &acc {
                            next.push(merge(p, q));
                        }
                    }
                    acc = next;
                }
                out.extend(acc);
            }
            out
        }
    }
}

pub fn report(auths: &[Authenticator], g: &GateExpr) -> GateReport {
    let ps = satisfying_paths(auths, g);
    GateReport {
        satisfiable: !ps.is_empty(),
        min_distinct_humans: ps.iter().map(|p| p.humans.len()).min(),
        zero_human_path: ps.iter().any(|p| p.humans.is_empty()),
        wait_alone_at: ps
            .iter()
            .filter(|p| p.humans.is_empty())
            .filter_map(|p| p.wait)
            .min(),
    }
}

fn named(g: &GateExpr) -> Vec<String> {
    fn factor_named(f: &Factor) -> Vec<String> {
        match f {
            Factor::Auth { id, .. } => vec![id.clone()],
            Factor::Group { expr, .. } => named(expr),
            _ => Vec::new(),
        }
    }
    match g {
        GateExpr::Single(f) => factor_named(f),
        GateExpr::Thresh { factors, .. } => factors.iter().flat_map(factor_named).collect(),
    }
}

/// Authenticator ids the gate names that the binding does not publish.
pub fn unknown_authenticators(auths: &[Authenticator], g: &GateExpr) -> Vec<String> {
    let unknown: Vec<String> = named(g)
        .into_iter()
        .filter(|i| !auths.iter().any(|a| a.id == *i))
        .collect();
    nub(&unknown)
}

/// Whether the requester's identity is an authenticator the gate names.
/// `humans()` counts the requester when the requester is a human
/// authenticator.
pub fn counts_requester(auths: &[Authenticator], requester: &str, g: &GateExpr) -> bool {
    let requester_human = auths.iter().any(|a| a.id == requester && a.human);
    fn factor(auths: &[Authenticator], requester: &str, requester_human: bool, f: &Factor) -> bool {
        match f {
            Factor::Auth { id, .. } => id == requester,
            Factor::Humans { .. } => requester_human,
            Factor::Group { expr, .. } => counts_requester(auths, requester, expr),
            Factor::Wait { .. } => false,
        }
    }
    match g {
        GateExpr::Single(f) => factor(auths, requester, requester_human, f),
        GateExpr::Thresh { factors, .. } => factors
            .iter()
            .any(|f| factor(auths, requester, requester_human, f)),
    }
}

/// The gate as the surface spells it.
pub fn render_gate(g: &GateExpr) -> String {
    fn weight(w: u32) -> String {
        if w == 1 {
            String::new()
        } else {
            format!(", weight: {w}")
        }
    }
    fn factor(f: &Factor) -> String {
        match f {
            Factor::Auth { id, weight: w } => format!("auth(:{id}{})", weight(*w)),
            Factor::Humans { weight: w } => {
                let ws = weight(*w);
                format!("humans({})", ws.get(2..).unwrap_or(""))
            }
            Factor::Group { expr, weight: w } => {
                format!("group({}{})", render_gate(expr), weight(*w))
            }
            Factor::Wait {
                duration,
                weight: w,
            } => format!("wait({}{})", duration.render(), weight(*w)),
        }
    }
    match g {
        GateExpr::Single(f) => factor(f),
        GateExpr::Thresh { n, factors } => {
            format!(
                "thresh({n}, {})",
                factors.iter().map(factor).collect::<Vec<_>>().join(", ")
            )
        }
    }
}
