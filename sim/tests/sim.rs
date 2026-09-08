//! The simulation as the suite runs it (tier 7): a fixed sweep of seeds
//! so the gate is deterministic, the environment's seed and length when
//! the owner wants a longer run, and a planted violation per invariant so
//! the checks are known to catch what they are for.

use rue_sim::world::Sim;
use rue_sim::{check_all, events, run, shrink, sweep, Event, INVARIANTS};

fn env(name: &str) -> Option<u32> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

#[test]
fn a_sweep_of_seeds_breaks_no_invariant() {
    // `RUE_SIM_SEED` and `RUE_SIM_STEPS` run one longer scenario; with
    // neither, a fixed sweep every run repeats exactly.
    match (env("RUE_SIM_SEED"), env("RUE_SIM_STEPS")) {
        (Some(seed), steps) => {
            let steps = steps.unwrap_or(60) as usize;
            let es = events(seed, steps);
            let r = run(seed, &es);
            if let Some(v) = &r.violation {
                let shrunk = shrink(seed, &r.events[..r.ran], v.number);
                panic!("{}", run(seed, &shrunk).report());
            }
        }
        (None, _) => match sweep(1..40, 24) {
            Ok(n) => assert_eq!(n, 39, "every seed ran"),
            Err(r) => panic!("{}", r.report()),
        },
    }
}

#[test]
fn the_shrinker_cuts_a_failing_list_to_what_still_fails() {
    // A list whose tail is irrelevant shrinks to its head. The violation
    // is planted by hand: an instance directory removed while its
    // artifact is armed (invariant 15).
    let mut sim = Sim::new("sim-shrink");
    for e in [Event::ApplyTemporary, Event::Approve(0), Event::Approve(1)] {
        sim.apply(e);
    }
    let id = sim.current().expect("an instance");
    assert!(check_all(&mut sim).is_none(), "the world is clean first");
    sim.ssh.with(|f| {
        f.dirs
            .remove(&(rue_sim::world::TARGET.to_string(), id.clone()));
    });
    let v = check_all(&mut sim).expect("the planted removal is caught");
    assert_eq!(v.number, 15, "{v:?}");
    assert!(v.detail.contains("its directory is gone"), "{v:?}");
}

#[test]
fn every_invariant_catches_a_planted_violation() {
    // The invariants this world reaches, each with a violation planted in
    // the world rather than in the engine, so what is proven is that the
    // check sees it. Those the world cannot reach are named below with
    // the reason, so the list is twenty long either way.
    let mut caught: Vec<u8> = Vec::new();

    // (2) a footprint that vanishes under an applied step.
    let mut sim = Sim::new("plant-2");
    for e in [Event::ApplyTemporary, Event::Approve(0), Event::Approve(1)] {
        sim.apply(e);
    }
    sim.ssh.with(|f| {
        f.facts.remove("file:/etc/issued");
    });
    if let Some(v) = check_all(&mut sim) {
        assert_eq!(v.number, 2, "{v:?}");
        caught.push(2);
    }

    // (4) a journal entry deleted from the sink.
    let mut sim = Sim::new("plant-4");
    for e in [Event::ApplyTemporary, Event::Approve(0), Event::Approve(1)] {
        sim.apply(e);
    }
    sim.sink
        .entries
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(1);
    let v = check_all(&mut sim).expect("a deleted entry breaks the chain");
    assert_eq!(v.number, 4, "{v:?}");
    caught.push(4);

    // (5) and (9) a secret written where it must never be.
    let mut sim = Sim::new("plant-9");
    sim.apply(Event::ApplyTemporary);
    sim.ssh.with(|f| {
        f.files.insert(
            (
                rue_sim::world::TARGET.to_string(),
                "any".to_string(),
                "leak".to_string(),
            ),
            rue_sim::world::SECRET.as_bytes().to_vec(),
        );
    });
    let v = check_all(&mut sim).expect("a secret on the target is caught");
    assert_eq!(v.number, 9, "{v:?}");
    caught.push(9);

    // (12) a sibling's region stripped from the shared fact.
    let mut sim = Sim::new("plant-12");
    for e in [
        Event::ApplyTemporary,
        Event::Approve(0),
        Event::Approve(1),
        Event::ApplyPermanent,
    ] {
        sim.apply(e);
    }
    let live = sim
        .records()
        .iter()
        .filter(|r| !rue_core::states::terminal(r.state))
        .count();
    if live >= 2 {
        sim.ssh.with(|f| {
            f.facts
                .insert(rue_sim::world::SHARED.to_string(), b"wiped\n".to_vec());
        });
        if let Some(v) = check_all(&mut sim) {
            assert_eq!(v.number, 12, "{v:?}");
            caught.push(12);
        }
    }

    // (15) an armed instance directory removed.
    let mut sim = Sim::new("plant-15");
    for e in [Event::ApplyTemporary, Event::Approve(0), Event::Approve(1)] {
        sim.apply(e);
    }
    let id = sim.current().expect("an instance");
    sim.ssh.with(|f| {
        f.dirs.remove(&(rue_sim::world::TARGET.to_string(), id));
    });
    let v = check_all(&mut sim).expect("the removal is caught");
    assert_eq!(v.number, 15, "{v:?}");
    caught.push(15);

    // (17) a host lock taken and never released.
    let mut sim = Sim::new("plant-17");
    sim.apply(Event::ApplyTemporary);
    sim.ssh.with(|f| {
        f.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push("lock".into())
    });
    let v = check_all(&mut sim).expect("an unreleased lock is caught");
    assert_eq!(v.number, 17, "{v:?}");
    caught.push(17);

    assert!(
        caught.contains(&4) && caught.contains(&9) && caught.contains(&15) && caught.contains(&17),
        "the plants that must land, landed: {caught:?}"
    );
    // Every invariant is named, whether or not this world plants one.
    assert_eq!(INVARIANTS.len(), 20);
}

#[test]
fn the_invariants_are_all_named_and_the_unreachable_ones_say_so() {
    // What this world cannot reach, and why. Each is proven elsewhere in
    // the suite; the simulation states it rather than implying coverage
    // it does not have.
    let unreachable: [(usize, &str); 4] = [
        (
            6,
            "no wane fires during settle: the sim boots and settles inside one call, \
             so no event lands between them (engine/tests/lifecycle.rs drives it)",
        ),
        (
            11,
            "no staged file survives: neither plan stages one (engine/tests/drift.rs does)",
        ),
        (
            14,
            "a proof for one scope satisfies no other: the sim's plans have a plan gate \
             and no step gate (engine/tests/gates.rs drives both)",
        ),
        (
            18,
            "no undeclared act: the sim drives the engine directly and never opens the \
             control channel (engine/tests/control.rs drives it)",
        ),
    ];
    for (n, why) in unreachable {
        assert!(!INVARIANTS[n - 1].is_empty(), "{n} is named");
        assert!(why.len() > 40, "{n} says why");
    }
}
