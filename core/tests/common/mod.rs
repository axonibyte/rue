//! Shared builders for the core tests: the prototype's `temp`, `owned`, `s`
//! helpers, and a seeded generator of knell-free plans for the laws.
#![allow(dead_code)]

use rue_core::body::*;
use rue_core::model::*;

pub mod gen;

pub fn owned(f: &str) -> Op {
    let shape = format!("file:/{f}");
    Op {
        do_: vec![write(fact_ref(&shape), lit("x"))],
        ..Op::new(f, vec![FootprintEntry::entry(Kind::Owned, &shape)])
    }
}

/// A computed undo body with the facts it needs unchanged.
pub fn computed(body: Vec<Prim>, undo_pre: &[&str]) -> Undo {
    Undo::Computed {
        body,
        undo_pre: undo_pre.iter().map(|s| s.to_string()).collect(),
    }
}

/// A compensating undo body with the facts it needs unchanged.
pub fn compensate(body: Vec<Prim>, undo_pre: &[&str]) -> Undo {
    Undo::Compensate {
        body,
        undo_pre: undo_pre.iter().map(|s| s.to_string()).collect(),
    }
}

pub fn modified(f: &str) -> Op {
    Op::new(
        f,
        vec![FootprintEntry::entry(Kind::Modified, &format!("file:/{f}"))],
    )
}

pub fn s(o: Op) -> Item {
    Item::Step(StepI::new(o))
}

/// A one-hour temporary plan on db-01 over the given items.
pub fn temp(items: Vec<Item>) -> Plan {
    Plan {
        wane: Some(Duration::new(3600)),
        ..Plan::new("p", "db-01", items)
    }
}

pub fn knell_op() -> Op {
    Op {
        undo: Undo::NoUndo,
        refusal: Refusal::Knell {
            guard: Some(Guard::new("verified_off", Tri::Yes)),
            cost: Cost::Probe("fence_verdict".into()),
            ack: Ack::NoAck("driver verified off".into()),
        },
        ..Op::new("fence", vec![])
    }
}
