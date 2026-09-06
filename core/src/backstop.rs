//! Backstops and ordering, docs/ROADMAP.md section 5.6: which steps a
//! backstop covers, where it is installed and armed, the late-arming window,
//! the reach rule, and the triggers an intent admits.

use crate::algebra::{numbered, op_of};
use crate::intent::{effective_wane, paths, Intent};
use crate::model::{Item, Plan, Trigger, UndoLocus};

/// What a plan's backstop covers, if it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// Steps with a `:target` undo, in order.
    pub covered: Vec<u32>,
    /// The first covered step.
    pub installed_before: Option<u32>,
    /// The plan's `arm_before`.
    pub armed_before_step: u32,
    /// Covered steps that complete before arming.
    pub late_arming_window: Vec<u32>,
}

fn target_undo_steps(items: &[Item]) -> Vec<u32> {
    numbered(items)
        .into_iter()
        .filter_map(|(n, it)| {
            op_of(it)
                .filter(|o| o.undo_locus == UndoLocus::Target)
                .map(|_| n)
        })
        .collect()
}

pub fn coverage(p: &Plan) -> Option<Coverage> {
    let b = p.backstop.as_ref()?;
    let cov = target_undo_steps(&p.body);
    let a = b.arm_before;
    Some(Coverage {
        installed_before: cov.first().copied(),
        late_arming_window: cov.iter().copied().filter(|n| *n < a).collect(),
        covered: cov,
        armed_before_step: a,
    })
}

/// Steps with `reach` that violate the reach rule (E0401): no `:target`
/// undo, no backstop, or a backstop armed after the step.
pub fn reach_violations(p: &Plan) -> Vec<u32> {
    let armed_before = |n: u32| match &p.backstop {
        None => false,
        Some(b) => b.arm_before <= n,
    };
    numbered(&p.body)
        .into_iter()
        .filter_map(|(n, it)| {
            let o = op_of(it)?;
            if !o.reach.is_empty() && (o.undo_locus != UndoLocus::Target || !armed_before(n)) {
                Some(n)
            } else {
                None
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerViolation {
    /// E0503: temporary plan's `after:` differs from its wane.
    AfterNotWane,
    /// E0503: `unless_confirmed` on a temporary plan that does not fire by construction.
    TemporaryConfirmed,
    /// E0504-adjacent: a permanent plan may not expire on a timer.
    PermanentAfter,
    /// E0504: paths reaching neither confirm nor commit.
    NoConfirmOrCommitPath(usize),
}

/// Trigger and path violations by intent.
pub fn trigger_violations(intent: Intent, p: &Plan) -> Vec<TriggerViolation> {
    let Some(b) = &p.backstop else {
        return Vec::new();
    };
    let afters: Vec<&Trigger> = b
        .triggers
        .iter()
        .filter(|t| matches!(t, Trigger::After(_)))
        .collect();
    match intent {
        Intent::Temporary if p.fires_by_construction => afters
            .iter()
            .map(|_| TriggerViolation::AfterNotWane)
            .collect(),
        Intent::Temporary => {
            let wane = effective_wane(p);
            let mut v: Vec<TriggerViolation> = b
                .triggers
                .iter()
                .filter(|t| matches!(t, Trigger::After(d) if Some(*d) != wane))
                .map(|_| TriggerViolation::AfterNotWane)
                .collect();
            v.extend(
                b.triggers
                    .iter()
                    .filter(|t| matches!(t, Trigger::UnlessConfirmed(_)))
                    .map(|_| TriggerViolation::TemporaryConfirmed),
            );
            if afters.is_empty() {
                v.push(TriggerViolation::AfterNotWane);
            }
            v
        }
        Intent::Permanent => {
            let mut v: Vec<TriggerViolation> = afters
                .iter()
                .map(|_| TriggerViolation::PermanentAfter)
                .collect();
            let k = paths_without_disarm(&p.body);
            if k > 0 && !p.fires_by_construction {
                v.push(TriggerViolation::NoConfirmOrCommitPath(k));
            }
            v
        }
    }
}

/// Paths that reach neither `confirm()` nor `commit()`.
fn paths_without_disarm(items: &[Item]) -> usize {
    paths(items)
        .iter()
        .filter(|path| {
            !path
                .iter()
                .any(|it| matches!(it, Item::Confirm | Item::Commit))
        })
        .count()
}

/// Heartbeat intervals above a third of their deadline (E0405).
pub fn heartbeat_violations(p: &Plan) -> Vec<Trigger> {
    match &p.backstop {
        None => Vec::new(),
        Some(b) => b
            .triggers
            .iter()
            .filter(|t| matches!(t, Trigger::UnlessHeartbeat { deadline, interval: Some(i) } if i.seconds * 3 > deadline.seconds))
            .cloned()
            .collect(),
    }
}
