//! Shared builders for the core tests: the prototype's `temp`, `owned`, `s`
//! helpers, and a seeded generator of knell-free plans for the laws.
#![allow(dead_code)]

use rue_core::body::*;
use rue_core::model::*;

/// xorshift32: deterministic across platforms, replayable from the seed.
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u32) as usize]
    }
}

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

fn gen_name(rng: &mut Rng) -> String {
    let n = 1 + rng.below(4);
    (0..n)
        .map(|_| *rng.pick(&['a', 'b', 'c', 'd', 'e', 'f']))
        .collect()
}

pub fn gen_step(rng: &mut Rng) -> StepI {
    let id = gen_name(rng);
    let kind = *rng.pick(&[Kind::Owned, Kind::Region, Kind::Modified]);
    let shape = format!("file:/{}", gen_name(rng));
    let mut st = StepI::new(Op::new(&id, vec![FootprintEntry::entry(kind, &shape)]));
    st.direction = *rng.pick(&[Direction::Forward, Direction::Inverse]);
    st
}

fn gen_guard(rng: &mut Rng) -> Guard {
    Guard::new(
        &gen_name(rng),
        *rng.pick(&[Tri::Yes, Tri::No, Tri::Unknown]),
    )
}

fn few<T>(rng: &mut Rng, n: u32, mut g: impl FnMut(&mut Rng) -> T) -> Vec<T> {
    let k = rng.below(n.min(4) + 1);
    (0..k).map(|_| g(rng)).collect()
}

/// A knell-free item of bounded size (the prototype's `genItem`).
pub fn gen_item(rng: &mut Rng, n: u32) -> Item {
    if n == 0 {
        return Item::Step(gen_step(rng));
    }
    match rng.below(12) {
        0..=5 => Item::Step(gen_step(rng)),
        6 => Item::Par {
            children: few(rng, n / 3, |r| gen_item(r, n / 3)),
        },
        7 => Item::Confirm,
        8 => Item::Observe {
            probe: gen_name(rng),
            alias: gen_name(rng),
        },
        9 => Item::Assert {
            guard: gen_guard(rng),
            window: None,
            on_lapse: OnLapse::Revert,
        },
        10 => Item::Repeat {
            form: RepeatForm::Count(2),
            var: "i".into(),
            body: few(rng, n / 3, |r| gen_item(r, n / 3)),
        },
        _ => Item::When {
            guard: gen_guard(rng),
            window: None,
            on_lapse: OnLapse::Revert,
            then_: few(rng, n / 3, |r| gen_item(r, n / 3)),
            else_: few(rng, n / 3, |r| gen_item(r, n / 3)),
        },
    }
}

/// A knell-free plan of modest depth.
pub fn gen_knell_free(rng: &mut Rng) -> Vec<Item> {
    let n = rng.below(9);
    few(rng, n, |r| gen_item(r, n))
}

pub fn gen_with_knell(rng: &mut Rng) -> Vec<Item> {
    let mut v = gen_knell_free(rng);
    v.push(Item::Knell(gen_step(rng)));
    v.extend(gen_knell_free(rng));
    v
}
