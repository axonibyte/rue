//! The interference query, docs/ROADMAP.md section 5.7, as iterator joins
//! shaped as the Datalog it is written in there:
//!
//! ```text
//! writes(S, F)       :- step(S), umbra(S, F).
//! maywrite(S, F)     :- step(S), penumbra(S, F).
//! needs(S, F)       :- step(S), undo_pre(S, F).
//! before(A, B)       :- lifo_order(A, B).          -- undefined between par siblings
//! conflict(A, B, F)  :- writes(A, F), writes(B, F), before(A, B), needs(A, F).
//! mayconflict(A,B,F) :- maywrite(A,F), writes(B,F), before(A,B), needs(A,F).
//! mayconflict(A,B,F) :- writes(A,F), maywrite(B,F), before(A,B), needs(A,F).
//! par_ok(P)          :- par(P), forall X,Y in children(P), X != Y => disjoint_umbra(X, Y).
//! ```
//!
//! Each relation is the function of that name; `ascent` would sit behind the
//! same signatures if scale ever demanded it (section 5.7, Phase 1 note).
//!
//! A fact is a footprint shape on a host: the host a step's locus resolves
//! to (the plan's owner for `:target`, the literal `controller` for
//! `:controller`, the named host for `host(...)`). The same shape on two hosts
//! is two facts (section 5.12). A shape whose text contains a runtime-bound
//! part, written `{...}`, is penumbral: its instance is bound only when a
//! value flows in; so is every fact of a step whose host is bound at runtime.
//! Region entries on one shape with distinct anchors are disjoint (section
//! 5.2); the same anchor twice in one plan is E0305. Iterations of one
//! `repeat over:` loop bind distinct instances of a shape indexed by the loop
//! variable and are disjoint by construction.
//!
//! Result order is the prototype's: pairs in leaf order, first occurrence
//! kept (`nub`); the verdict's bytes depend on it.

use crate::model::{HostRef, Item, Kind, Locus, Op, RepeatForm, Undo};
use crate::util::nub;

/// A fact as the query sees it: a shape, and for a region its anchor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fact {
    pub shape: String,
    pub anchor: Option<String>,
}

impl Fact {
    pub fn new(shape: &str, anchor: Option<&str>) -> Fact {
        Fact {
            shape: shape.to_string(),
            anchor: anchor.map(str::to_string),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Conflict {
    pub earlier: u32,
    pub later: u32,
    pub fact: Fact,
}

/// A numbered step as the query sees it: its op, the host its facts live on,
/// and the loop variables whose iterations make a shape distinct by
/// construction.
#[derive(Debug, Clone)]
pub struct Leaf<'a> {
    pub n: u32,
    pub op: &'a Op,
    pub host: String,
    pub vars: Vec<String>,
}

/// A shape whose instance is bound only at runtime.
fn runtime_bound(s: &str) -> bool {
    s.contains('{')
}

fn kind_writes(k: Kind) -> bool {
    !matches!(k, Kind::Derived)
}

/// `writes(S, F)`: the facts an op definitely writes (its umbra).
pub fn writes(o: &Op) -> Vec<Fact> {
    o.footprint
        .iter()
        .filter(|e| kind_writes(e.kind) && !runtime_bound(&e.shape))
        .map(|e| Fact {
            shape: e.shape.clone(),
            anchor: e.anchor.clone(),
        })
        .collect()
}

/// `maywrite(S, F)`: the facts an op may write (its penumbra): written kinds
/// whose shape is runtime-bound.
pub fn maywrite(o: &Op) -> Vec<Fact> {
    o.footprint
        .iter()
        .filter(|e| kind_writes(e.kind) && runtime_bound(&e.shape))
        .map(|e| Fact {
            shape: e.shape.clone(),
            anchor: e.anchor.clone(),
        })
        .collect()
}

/// `needs(S, F)`: what an op's undo needs unchanged. `Restore` derives it
/// from the footprint: every fact it wrote must still equal its post-do
/// value. Computed and compensating undos declare it.
pub fn needs(o: &Op) -> Vec<Fact> {
    match &o.undo {
        Undo::Restore => {
            let mut v = writes(o);
            v.extend(maywrite(o));
            v
        }
        Undo::Computed(pre) | Undo::Compensate(pre) => {
            pre.iter().map(|s| Fact::new(s, None)).collect()
        }
        Undo::NoUndo => Vec::new(),
    }
}

/// Two facts touch when their shapes overlap and, for two regions, their
/// anchors are equal; a region and a non-region on one shape touch. Two
/// static shapes overlap when equal; a runtime-bound shape overlaps anything
/// that shares its static prefix, which is what makes it penumbral.
fn touches(f: &Fact, g: &Fact) -> bool {
    if !overlap_shape(&f.shape, &g.shape) {
        return false;
    }
    match (&f.anchor, &g.anchor) {
        (Some(x), Some(y)) => x == y,
        _ => true,
    }
}

fn overlap_shape(s1: &str, s2: &str) -> bool {
    if !runtime_bound(s1) && !runtime_bound(s2) {
        return s1 == s2;
    }
    let static_part = |s: &str| -> String { s.chars().take_while(|c| *c != '{').collect() };
    s2.starts_with(static_part(s1).as_str()) || s1.starts_with(static_part(s2).as_str())
}

/// The host a step's locus resolves to, as text; a bound host is written
/// `{name}` so that it is runtime-bound like a penumbral shape.
pub fn leaf_host_text(owner: &str, o: &Op) -> String {
    match &o.locus {
        Locus::Controller => "controller".to_string(),
        Locus::Target => owner.to_string(),
        Locus::Host(HostRef::Static(h)) => h.clone(),
        Locus::Host(HostRef::Bound(b)) => format!("{{{b}}}"),
    }
}

/// Two hosts overlap when equal, or when either is bound at runtime.
fn overlap_host(h1: &str, h2: &str) -> bool {
    h1 == h2 || runtime_bound(h1) || runtime_bound(h2)
}

fn same_host(x: &Leaf<'_>, y: &Leaf<'_>) -> bool {
    overlap_host(&x.host, &y.host)
}

fn bound_host(x: &Leaf<'_>) -> bool {
    runtime_bound(&x.host)
}

fn leaf_count(items: &[Item]) -> u32 {
    items
        .iter()
        .map(|it| match it {
            Item::Par { children } => leaf_count(children),
            Item::Repeat { body, .. } => leaf_count(body),
            Item::When { then_, else_, .. } => leaf_count(then_) + leaf_count(else_),
            _ => 1,
        })
        .sum()
}

/// `step(S)`: the numbered steps of a plan owned by the given host.
pub fn step_facts<'a>(owner: &str, items: &'a [Item]) -> Vec<Leaf<'a>> {
    fn go<'a>(
        owner: &str,
        vars: &[String],
        items: &'a [Item],
        start: u32,
        out: &mut Vec<Leaf<'a>>,
    ) {
        let mut n = start;
        for it in items {
            match it {
                Item::Par { children } => {
                    go(owner, vars, children, n, out);
                    n += leaf_count(children);
                }
                Item::Repeat {
                    form: RepeatForm::Over { .. },
                    var,
                    body,
                } => {
                    let mut inner = vec![var.clone()];
                    inner.extend_from_slice(vars);
                    go(owner, &inner, body, n, out);
                    n += leaf_count(body);
                }
                Item::Repeat {
                    form: RepeatForm::Count(_),
                    body,
                    ..
                } => {
                    go(owner, vars, body, n, out);
                    n += leaf_count(body);
                }
                Item::When { then_, else_, .. } => {
                    go(owner, vars, then_, n, out);
                    go(owner, vars, else_, n + leaf_count(then_), out);
                    n += leaf_count(then_) + leaf_count(else_);
                }
                Item::Step(s) | Item::Knell(s) => {
                    out.push(Leaf {
                        n,
                        op: &s.op,
                        host: leaf_host_text(owner, &s.op),
                        vars: vars.to_vec(),
                    });
                    n += 1;
                }
                _ => n += 1,
            }
        }
    }
    let mut out = Vec::new();
    go(owner, &[], items, 1, &mut out);
    out
}

/// A fact indexed by a loop variable of its own iteration is distinct across
/// iterations; two such facts from the same loop body are disjoint.
fn indexed_by(vars: &[String], f: &Fact) -> bool {
    vars.iter().any(|v| f.shape.contains(&format!("{{{v}}}")))
}

fn same_loop(va: &[String], vb: &[String]) -> bool {
    !va.is_empty() && va == vb
}

/// `before(A, B)`: leaf order, undefined between children of one `par`.
pub fn before(x: &Leaf<'_>, y: &Leaf<'_>, siblings: &[(u32, u32)]) -> bool {
    x.n < y.n && !siblings.contains(&(x.n, y.n))
}

/// Ordered pairs of leaves with `before` between them, in leaf order.
fn ordered_pairs<'l, 'a>(
    leaves: &'l [Leaf<'a>],
    siblings: &[(u32, u32)],
) -> Vec<(&'l Leaf<'a>, &'l Leaf<'a>)> {
    let mut out = Vec::new();
    for (i, x) in leaves.iter().enumerate() {
        for y in &leaves[i + 1..] {
            if before(x, y, siblings) {
                out.push((x, y));
            }
        }
    }
    out
}

/// `conflict(A, B, F)` (E0301): an earlier step's undo needs a fact a later
/// step also definitely writes, on one statically known host. Two children
/// of one `par` have no order between them and are judged by `par_ok`.
pub fn conflict(owner: &str, items: &[Item]) -> Vec<Conflict> {
    let leaves = step_facts(owner, items);
    let siblings = par_siblings(owner, items);
    let mut out = Vec::new();
    for (x, y) in ordered_pairs(&leaves, &siblings) {
        if bound_host(x) || bound_host(y) || x.host != y.host {
            continue;
        }
        let needs_a = needs(x.op);
        for f in writes(x.op) {
            if !needs_a.contains(&f) {
                continue;
            }
            for g in writes(y.op) {
                if touches(&f, &g) && !(same_loop(&x.vars, &y.vars) && indexed_by(&x.vars, &f)) {
                    out.push(Conflict {
                        earlier: x.n,
                        later: y.n,
                        fact: f.clone(),
                    });
                }
            }
        }
    }
    nub(&out)
}

/// `mayconflict(A, B, F)` (E0302 under strict, a verdict clause under warn):
/// the same query where at least one side is penumbral, by shape or by host.
pub fn mayconflict(owner: &str, items: &[Item]) -> Vec<Conflict> {
    let leaves = step_facts(owner, items);
    let siblings = par_siblings(owner, items);
    let mut out = Vec::new();
    for (x, y) in ordered_pairs(&leaves, &siblings) {
        if !same_host(x, y) {
            continue;
        }
        let pen_a = maywrite(x.op);
        let pen_b = maywrite(y.op);
        let mut all_a = pen_a.clone();
        all_a.extend(writes(x.op));
        let mut all_b = pen_b.clone();
        all_b.extend(writes(y.op));
        for f in needs(x.op) {
            for g in &all_a {
                if !touches(&f, g) {
                    continue;
                }
                for h in &all_b {
                    if !touches(&f, h) {
                        continue;
                    }
                    if !(pen_b.contains(h) || pen_a.contains(g) || bound_host(x) || bound_host(y)) {
                        continue;
                    }
                    if same_loop(&x.vars, &y.vars) && indexed_by(&x.vars, &f) {
                        continue;
                    }
                    out.push(Conflict {
                        earlier: x.n,
                        later: y.n,
                        fact: f.clone(),
                    });
                }
            }
        }
    }
    nub(&out)
}

/// `disjoint_umbra(X, Y)`: pairwise disjoint umbras, trivially so on
/// distinct static hosts.
pub fn disjoint_umbra(owner: &str, x: &Op, y: &Op) -> bool {
    !overlap_host(&leaf_host_text(owner, x), &leaf_host_text(owner, y))
        || writes(x)
            .iter()
            .all(|f| writes(y).iter().all(|g| !touches(f, g)))
}

/// Par violations: pairs of children whose umbras overlap (E0303), and any
/// child with `reach` (E0304). Each child's ops are collected recursively.
pub fn par_violations(owner: &str, items: &[Item]) -> (Vec<(u32, u32)>, Vec<u32>) {
    let groups = par_children(owner, items);
    let mut overlaps = Vec::new();
    let mut reachy = Vec::new();
    for grp in &groups {
        for (i, ca) in grp.iter().enumerate() {
            for cb in &grp[i + 1..] {
                for (a, oa) in ca {
                    for (b, ob) in cb {
                        if !disjoint_umbra(owner, oa, ob) {
                            overlaps.push((*a, *b));
                        }
                    }
                }
            }
        }
        for child in grp {
            for (n, o) in child {
                if !o.reach.is_empty() {
                    reachy.push(*n);
                }
            }
        }
    }
    (nub(&overlaps), nub(&reachy))
}

/// `par_ok(P)` for every par in the plan: no overlapping children and no
/// `reach` inside.
pub fn par_ok(owner: &str, items: &[Item]) -> bool {
    let (overlaps, reachy) = par_violations(owner, items);
    overlaps.is_empty() && reachy.is_empty()
}

/// Pairs of steps (earlier, later) that sit in different children of one
/// `par` and so have no order between them.
pub fn par_siblings(owner: &str, items: &[Item]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for grp in par_children(owner, items) {
        for (i, ca) in grp.iter().enumerate() {
            for cb in &grp[i + 1..] {
                for (a, _) in ca {
                    for (b, _) in cb {
                        out.push((*a.min(b), *a.max(b)));
                    }
                }
            }
        }
    }
    nub(&out)
}

/// Every `par` in the plan, as its children, each child its (step, op)s.
/// Order is the prototype's: a par, then the pars after it, then the pars
/// nested inside its children.
fn par_children<'a>(owner: &str, items: &'a [Item]) -> Vec<Vec<Vec<(u32, &'a Op)>>> {
    let leaves = step_facts(owner, items);
    collect_pars(&leaves, items, 1)
}

/// The recursion behind `par_children`, transcribing the prototype's
/// `collect`: at a par, its group, then the pars after it, then the pars
/// inside each child.
fn collect_pars<'a>(
    leaves: &[Leaf<'a>],
    items: &'a [Item],
    start: u32,
) -> Vec<Vec<Vec<(u32, &'a Op)>>> {
    let mut out = Vec::new();
    let mut n = start;
    for (idx, item) in items.iter().enumerate() {
        match item {
            Item::Par { children } => {
                let mut starts = Vec::new();
                let mut k = n;
                for c in children {
                    starts.push(k);
                    k += leaf_count(std::slice::from_ref(c));
                }
                let group: Vec<Vec<(u32, &'a Op)>> = children
                    .iter()
                    .zip(starts.iter())
                    .map(|(c, &k)| {
                        let end = k + leaf_count(std::slice::from_ref(c));
                        leaves
                            .iter()
                            .filter(|l| l.n >= k && l.n < end)
                            .map(|l| (l.n, l.op))
                            .collect()
                    })
                    .collect();
                out.push(group);
                let after = collect_pars(leaves, &items[idx + 1..], n + leaf_count(children));
                out.extend(after);
                for (c, &k) in children.iter().zip(starts.iter()) {
                    out.extend(collect_pars(leaves, std::slice::from_ref(c), k));
                }
                return out;
            }
            Item::Repeat { body, .. } => {
                out.extend(collect_pars(leaves, body, n));
                n += leaf_count(body);
            }
            Item::When { then_, else_, .. } => {
                out.extend(collect_pars(leaves, then_, n));
                out.extend(collect_pars(leaves, else_, n + leaf_count(then_)));
                n += leaf_count(then_) + leaf_count(else_);
            }
            _ => n += 1,
        }
    }
    out
}

/// The same anchor declared twice on one fact within a plan (E0305): the
/// offending (step, step, fact) triples.
pub fn anchor_duplicates(owner: &str, items: &[Item]) -> Vec<Conflict> {
    let leaves = step_facts(owner, items);
    let regions = |o: &Op| -> Vec<Fact> {
        o.footprint
            .iter()
            .filter(|e| e.kind == Kind::Region)
            .map(|e| Fact {
                shape: e.shape.clone(),
                anchor: e.anchor.clone(),
            })
            .collect()
    };
    let mut out = Vec::new();
    for (i, x) in leaves.iter().enumerate() {
        for y in &leaves[i + 1..] {
            if !same_host(x, y) {
                continue;
            }
            for f in regions(x.op) {
                if f.anchor.is_none() {
                    continue;
                }
                for g in regions(y.op) {
                    if f == g {
                        out.push(Conflict {
                            earlier: x.n,
                            later: y.n,
                            fact: f.clone(),
                        });
                    }
                }
            }
        }
    }
    nub(&out)
}
