//! Intent, docs/ROADMAP.md section 5.4: a plan with `wane` is temporary; a
//! plan with a reachable `commit()` is permanent; both or neither is E0501,
//! except a plan declaring `fires_by_construction`, which is temporary with
//! its `unless_confirmed` duration as `wane`. A permanent plan reaches
//! `commit()` on every non-refusing path (E0505) and `commit()` is the last
//! item on its path (E0502).

use crate::algebra::numbered;
use crate::model::{Duration, Item, Plan, Trigger};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Intent {
    Temporary,
    Permanent,
}

/// The intent, or `None` when it cannot be determined (E0501).
pub fn infer_intent(p: &Plan) -> Option<Intent> {
    let has_wane = p.wane.is_some();
    let has_commit = commit_reachable(&p.body);
    if p.fires_by_construction {
        if has_wane || has_commit {
            None
        } else {
            Some(Intent::Temporary)
        }
    } else if has_wane && !has_commit {
        Some(Intent::Temporary)
    } else if has_commit && !has_wane {
        Some(Intent::Permanent)
    } else {
        None
    }
}

/// The bound a temporary plan's states expire at: `wane`, or for a
/// fires-by-construction plan the `unless_confirmed` duration.
pub fn effective_wane(p: &Plan) -> Option<Duration> {
    match p.wane {
        Some(w) => Some(w),
        None if p.fires_by_construction => p.backstop.as_ref().and_then(|b| {
            b.triggers.iter().find_map(|t| match t {
                Trigger::UnlessConfirmed(d) => Some(*d),
                _ => None,
            })
        }),
        None => None,
    }
}

pub fn commit_reachable(items: &[Item]) -> bool {
    items.iter().any(|it| match it {
        Item::Commit => true,
        Item::Par { children } => commit_reachable(children),
        Item::Repeat { body, .. } => commit_reachable(body),
        Item::When { then_, else_, .. } => commit_reachable(then_) || commit_reachable(else_),
        _ => false,
    })
}

/// The step number of the first `commit()`, if any.
pub fn commit_step(items: &[Item]) -> Option<u32> {
    numbered(items)
        .into_iter()
        .find(|(_, it)| matches!(it, Item::Commit))
        .map(|(n, _)| n)
}

/// Every path through a plan: the sequence of leaves along each choice of
/// `when` arm. Repeat bodies contribute once; par children contribute in
/// order.
pub fn paths(items: &[Item]) -> Vec<Vec<&Item>> {
    let mut acc: Vec<Vec<&Item>> = vec![Vec::new()];
    for it in items {
        let here: Vec<Vec<&Item>> = match it {
            Item::Par { children } => paths(children),
            Item::Repeat { body, .. } => paths(body),
            Item::When { then_, else_, .. } => {
                let mut v = paths(then_);
                v.extend(paths(else_));
                v
            }
            _ => vec![vec![it]],
        };
        let mut next = Vec::with_capacity(acc.len() * here.len());
        for prefix in &acc {
            for h in &here {
                let mut path = prefix.clone();
                path.extend_from_slice(h);
                next.push(path);
            }
        }
        acc = next;
    }
    acc
}

/// Paths on which `commit()` is followed by another item (E0502).
pub fn commit_not_last(items: &[Item]) -> bool {
    paths(items).iter().any(|path| {
        let after: Vec<&&Item> = path
            .iter()
            .skip_while(|it| !matches!(it, Item::Commit))
            .collect();
        after.len() >= 2
    })
}

/// Paths that never reach `commit()` (E0505 for a permanent plan). A path
/// ending in a knell-free refusal is still a path; the roadmap's
/// "non-refusing path" is every path the checker can enumerate, since refusal
/// is a runtime outcome, not a syntactic one.
pub fn paths_without_commit(items: &[Item]) -> usize {
    paths(items)
        .iter()
        .filter(|path| !path.iter().any(|it| matches!(it, Item::Commit)))
        .count()
}
