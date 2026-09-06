//! The reversal algebra, docs/ROADMAP.md section 5.5.
//!
//! Laws (held in `tests/laws.rs`):
//!
//! ```text
//! reverse (seq a b) = seq (reverse b) (reverse a)
//! reverse (par xs)  = par (map reverse xs)
//! reverse knell     = refuse
//! reverse . reverse = id            -- on knell-free plans
//! ```
//!
//! Reversal is syntactic: a step's direction flips, a sequence's order flips,
//! a `par` reverses each child in place, and a knell refuses. Partial
//! reversal, `reverse_from k`, undoes the applied prefix last-in-first-out; it
//! is the operation the runtime actually performs.

use crate::model::{Direction, Item, Op, StepI};

/// Why a reversal is refused: the knell at this position is irreversible.
/// Boxed so a `Result` carrying it stays small on the `Ok` path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub at: Box<Item>,
}

/// `seq` is list append; `a |> b` desugars to it and nothing else.
pub fn seq_(a: &[Item], b: &[Item]) -> Vec<Item> {
    let mut out = a.to_vec();
    out.extend_from_slice(b);
    out
}

pub fn par_(children: Vec<Item>) -> Item {
    Item::Par { children }
}

fn flip_step(s: &StepI) -> StepI {
    let mut t = s.clone();
    t.direction = match s.direction {
        Direction::Forward => Direction::Inverse,
        Direction::Inverse => Direction::Forward,
    };
    t
}

/// Reverse one item. Non-mutating items (confirm, commit, observe, assert,
/// preflight, a slot) reverse to themselves: they have no undo because they
/// changed nothing.
pub fn reverse_item(it: &Item) -> Result<Item, Refused> {
    match it {
        Item::Step(s) => Ok(Item::Step(flip_step(s))),
        Item::Par { children } => Ok(Item::Par {
            children: children
                .iter()
                .map(reverse_item)
                .collect::<Result<_, _>>()?,
        }),
        Item::Knell(_) => Err(Refused {
            at: Box::new(it.clone()),
        }),
        Item::Repeat { form, var, body } => Ok(Item::Repeat {
            form: form.clone(),
            var: var.clone(),
            body: reverse_items(body)?,
        }),
        Item::When {
            guard,
            window,
            on_lapse,
            then_,
            else_,
        } => Ok(Item::When {
            guard: guard.clone(),
            window: *window,
            on_lapse: *on_lapse,
            then_: reverse_items(then_)?,
            else_: reverse_items(else_)?,
        }),
        Item::Slot { .. }
        | Item::Confirm
        | Item::Commit
        | Item::Preflight { .. }
        | Item::Observe { .. }
        | Item::Assert { .. } => Ok(it.clone()),
    }
}

/// Reverse a sequence: last-in-first-out, each item reversed.
pub fn reverse_items(items: &[Item]) -> Result<Vec<Item>, Refused> {
    items.iter().rev().map(reverse_item).collect()
}

/// Undo the applied prefix of a plan: the first `k` leaves, in reverse.
pub fn reverse_from(k: usize, items: &[Item]) -> Result<Vec<Item>, Refused> {
    let prefix: Vec<Item> = leaves(items).into_iter().take(k).cloned().collect();
    reverse_items(&prefix)
}

/// A plan with no knell anywhere in it.
pub fn knell_free(items: &[Item]) -> bool {
    items.iter().all(|it| match it {
        Item::Knell(_) => false,
        Item::Par { children } => knell_free(children),
        Item::Repeat { body, .. } => knell_free(body),
        Item::When { then_, else_, .. } => knell_free(then_) && knell_free(else_),
        _ => true,
    })
}

/// The leaves of a plan in execution order: containers (par, repeat, when)
/// contribute their children; everything else is a leaf.
pub fn leaves(items: &[Item]) -> Vec<&Item> {
    let mut out = Vec::new();
    fn go<'a>(it: &'a Item, out: &mut Vec<&'a Item>) {
        match it {
            Item::Par { children } => children.iter().for_each(|c| go(c, out)),
            Item::Repeat { body, .. } => body.iter().for_each(|c| go(c, out)),
            Item::When { then_, else_, .. } => {
                then_.iter().for_each(|c| go(c, out));
                else_.iter().for_each(|c| go(c, out));
            }
            _ => out.push(it),
        }
    }
    items.iter().for_each(|it| go(it, &mut out));
    out
}

/// The leaves numbered from 1, which is how the verdict and `explain` refer
/// to steps.
pub fn numbered(items: &[Item]) -> Vec<(u32, &Item)> {
    leaves(items)
        .into_iter()
        .zip(1u32..)
        .map(|(it, n)| (n, it))
        .collect()
}

/// The step, if the leaf is a step or a knell.
pub fn step_of(it: &Item) -> Option<&StepI> {
    match it {
        Item::Step(s) | Item::Knell(s) => Some(s),
        _ => None,
    }
}

/// The step's op, if the leaf is a step or a knell.
pub fn op_of(it: &Item) -> Option<&Op> {
    step_of(it).map(|s| &s.op)
}
