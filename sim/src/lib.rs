//! rue-sim: the shadow world of docs/ROADMAP.md 10 (tier 7).
//!
//! A run is a seed and a number of steps. The seed makes an event list;
//! the events are fed to a real engine over the fake executor, the fake
//! scheduler and the fake clock; after every event the twenty invariants
//! of 10.3 are checked against what the engine, its fakes, its journal
//! and its ledger say. A run that breaks one is shrunk by delta debugging
//! to the shortest prefix-and-subset of events that still breaks it, and
//! reported with its seed, so a failure is a reproduction rather than an
//! anecdote.
//!
//! The world is deliberately small: two hosts, one temporary plan and one
//! permanent one, cut from the shapes T1 and T3 have. What it is for is
//! not coverage of the language but the orderings a single run cannot
//! reach: an approval and a wane in the wrong order, a boot in the middle
//! of an undo, an artifact firing while an operator recants.
//!
//! `RUE_SIM_SEED` and `RUE_SIM_STEPS` set the seed and the length; with
//! neither, the tests run a fixed sweep so the suite is deterministic.

pub mod invariants;
pub mod world;

use std::fmt;

pub use invariants::{check_all, Violation};
pub use world::{Sim, INVARIANTS};

/// xorshift32, the generator every seeded test in rue uses, so a run is
/// replayable from its seed on any platform.
pub struct Rng(pub u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(if seed == 0 { 0x5eed_1eaf } else { seed })
    }
    /// The next word. Named for what it is; the lint that reads it as
    /// `Iterator::next` is answered here rather than by renaming the
    /// generator every other seeded test in rue already calls `next`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

/// What can happen to a running site. Every one is something an operator,
/// a target or the world does; none of them reaches inside the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Request the temporary plan.
    ApplyTemporary,
    /// Request the permanent plan.
    ApplyPermanent,
    /// A proof for the plan gate, from the named authenticator.
    Approve(u8),
    /// Time passes.
    Tick(u64),
    /// A reap pass.
    Reap,
    /// The daemon stops and starts again: boot recovery and settle.
    Reboot,
    /// An operator recants the instance.
    Recant,
    /// An operator recants, forcing drift.
    RecantForcingDrift,
    /// Someone edits a fact the plan owns, behind the engine's back.
    EditFact,
    /// The target's artifact fires.
    Fire,
    /// The scheduler entry is removed behind the engine's back.
    LoseSchedulerEntry,
    /// An operator confirms a permanent plan.
    Confirm,
    /// An operator commits a permanent plan.
    Commit,
    /// An operator abandons a stuck instance.
    Abandon,
    /// The next executor call fails.
    BreakExecutor,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Tick(s) => write!(f, "Tick({s})"),
            Event::Approve(a) => write!(f, "Approve({a})"),
            other => write!(f, "{other:?}"),
        }
    }
}

const KINDS: &[Event] = &[
    Event::ApplyTemporary,
    Event::ApplyPermanent,
    Event::Approve(0),
    Event::Approve(1),
    Event::Tick(0),
    Event::Reap,
    Event::Reboot,
    Event::Recant,
    Event::RecantForcingDrift,
    Event::EditFact,
    Event::Fire,
    Event::LoseSchedulerEntry,
    Event::Confirm,
    Event::Commit,
    Event::Abandon,
    Event::BreakExecutor,
];

/// An event list from a seed.
pub fn events(seed: u32, steps: usize) -> Vec<Event> {
    let mut rng = Rng::new(seed);
    let mut v = Vec::with_capacity(steps);
    // A run that never applies anything proves nothing, so it starts with
    // one of the two plans and the rest is the generator's.
    v.push(if rng.below(2) == 0 {
        Event::ApplyTemporary
    } else {
        Event::ApplyPermanent
    });
    while v.len() < steps {
        let e = KINDS[rng.below(KINDS.len() as u32) as usize];
        v.push(match e {
            Event::Tick(_) => Event::Tick(u64::from(rng.below(4)) * 900 + 60),
            other => other,
        });
    }
    v
}

/// What a run found: the events it ran and the first violation, if any.
#[derive(Debug)]
pub struct Run {
    pub seed: u32,
    pub events: Vec<Event>,
    pub violation: Option<Violation>,
    /// How many events ran before the violation (or all of them).
    pub ran: usize,
}

impl Run {
    pub fn ok(&self) -> bool {
        self.violation.is_none()
    }

    /// The line a failing run prints: the seed, the shrunk events and the
    /// invariant.
    pub fn report(&self) -> String {
        match &self.violation {
            None => format!("seed {}: {} events, clean", self.seed, self.events.len()),
            Some(v) => format!(
                "seed {}: invariant {} ({}) broken after {} events: {}\n  events: {}",
                self.seed,
                v.number,
                v.name,
                self.ran,
                v.detail,
                self.events[..self.ran]
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// Run an event list against a fresh world, checking every invariant
/// after every event. The first violation ends the run.
pub fn run(seed: u32, events: &[Event]) -> Run {
    let mut sim = Sim::new(&format!("sim-{seed}"));
    for (i, e) in events.iter().enumerate() {
        sim.apply(*e);
        if let Some(v) = check_all(&mut sim) {
            return Run {
                seed,
                events: events.to_vec(),
                violation: Some(v),
                ran: i + 1,
            };
        }
    }
    Run {
        seed,
        events: events.to_vec(),
        violation: None,
        ran: events.len(),
    }
}

/// Delta debugging over the event list: the shortest list that still
/// breaks the same invariant. Each candidate is a fresh world, so a
/// shorter list that breaks a *different* invariant is not accepted.
pub fn shrink(seed: u32, failing: &[Event], number: u8) -> Vec<Event> {
    let breaks_same =
        |es: &[Event]| -> bool { run(seed, es).violation.is_some_and(|v| v.number == number) };
    let mut best = failing.to_vec();
    let mut changed = true;
    while changed {
        changed = false;
        // Drop one event at a time, keeping the first list that still
        // breaks the same invariant.
        let mut i = 0;
        while i < best.len() {
            let mut candidate = best.clone();
            candidate.remove(i);
            if breaks_same(&candidate) {
                best = candidate;
                changed = true;
            } else {
                i += 1;
            }
        }
    }
    best
}

/// A sweep: every seed in the range, `steps` events each, stopping at the
/// first failure with the shrunk list.
pub fn sweep(seeds: std::ops::Range<u32>, steps: usize) -> Result<usize, Run> {
    let mut ran = 0;
    for seed in seeds {
        let es = events(seed, steps);
        let r = run(seed, &es);
        ran += 1;
        if let Some(v) = &r.violation {
            let shrunk = shrink(seed, &r.events[..r.ran], v.number);
            let again = run(seed, &shrunk);
            return Err(Run {
                seed,
                ran: again.ran,
                events: shrunk,
                violation: again.violation.or(r.violation),
            });
        }
    }
    Ok(ran)
}
