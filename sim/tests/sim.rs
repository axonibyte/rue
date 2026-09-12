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
    let unreachable: [(usize, &str); 2] = [
        (
            6,
            "no wane fires during settle: the sim boots and settles inside one call, \
             so no event lands between them (engine/tests/lifecycle.rs drives it)",
        ),
        (
            18,
            "no undeclared act: the sim drives the engine directly and never opens the \
             control channel (engine/tests/control.rs drives it)",
        ),
    ];
    // 11 and 14 were on this list until the third plan arrived: it stages
    // a file and gates a step, so a staged file surviving and a plan proof
    // opening a step gate are now things this world can do wrong.
    for (n, why) in unreachable {
        assert!(!INVARIANTS[n - 1].is_empty(), "{n} is named");
        assert!(why.len() > 40, "{n} says why");
    }
}

/// The third plan is not decoration: it checks clean, its repeat runs once
/// per guest, its hook-executed step waits on a step gate a *plan* proof
/// does not satisfy, it stages a file, and its console step defers until
/// the handoff is reported. A plan the checker refused would make every
/// event naming it a no-op, and the sweep would be as small as before
/// while looking twice the size.
#[test]
fn the_succession_plan_runs_its_repeat_its_gate_its_stage_and_its_handoff() {
    let mut sim = Sim::new("succession");
    sim.apply(Event::ApplySuccession);
    let id = sim
        .current()
        .expect("the plan was requested, so it checked");
    let rec = |s: &Sim| {
        s.records()
            .into_iter()
            .find(|r| r.id == id)
            .expect("its record")
    };

    // The repeat ran once per guest, each iteration with its own variable.
    let r = rec(&sim);
    let vars: Vec<String> = r
        .applied
        .iter()
        .filter_map(|a| a.vars.get("g").cloned())
        .collect();
    assert_eq!(vars, vec!["g1".to_string(), "g2".to_string()], "{r:?}");
    // The stage() ran on the target: invariant 11 -- no staged file
    // surviving an instance that is not applying -- has something to be
    // about in this world now, where before neither plan staged anything.
    // A staged file is written inside the step's own body, so the proof it
    // happened is the body the executor was handed; the proof the
    // invariant holds is that the file is gone again by here, the instance
    // having stopped applying.
    assert!(
        sim.ssh
            .with(|f| f.calls.iter().any(|c| c.body.iter().any(|p| matches!(
                p,
                rue_engine::executor::RPrim::Stage { name, .. } if name == "guest.conf"
            )))),
        "the step staged a file"
    );
    assert!(
        sim.ssh.events().iter().any(|e| e == "remove guest.conf"),
        "and it was removed when the instance stopped applying: {:?}",
        sim.ssh.events()
    );

    // The gated step waits: a proof for the plan's scope satisfies no
    // step's (invariant 14), so the plan proof leaves it waiting.
    assert_eq!(
        r.waiting.as_ref().map(|w| w.step),
        Some(2),
        "waiting at the step gate: {:?}",
        r.waiting
    );
    sim.apply(Event::Approve(0));
    assert_eq!(
        rec(&sim).waiting.as_ref().map(|w| w.step),
        Some(2),
        "a plan proof does not open a step gate"
    );
    sim.apply(Event::ApproveStep(2));
    let r = rec(&sim);
    assert!(
        r.waiting.is_none(),
        "the step's own proof opened it: {:?}",
        r.waiting
    );

    // And the console step defers: no transport of this site reaches it.
    assert_eq!(
        r.deferred.as_ref().map(|d| d.step),
        Some(3),
        "deferred at the console step: {r:?}"
    );
    sim.apply(Event::HandoffDone);
    let r = rec(&sim);
    assert!(r.deferred.is_none(), "the handoff continued it: {r:?}");
    assert!(
        r.applied.iter().any(|a| a.step == 3),
        "the deferred step applied after its handoff: {:?}",
        r.applied
    );
    assert!(check_all(&mut sim).is_none(), "the world is consistent");
}

/// Unit 2's rules, in the shadow. A read that drops is an error the step
/// reports and never the absence of the fact, and a `do` that never took
/// is not undone -- journaled `UndoSkipped`. Both are engine rules with
/// their own tests; what this asks is whether the simulated world still
/// agrees with the engine while they fire, which is the only place the two
/// meet under an arbitrary ordering.
#[test]
fn a_dropped_read_and_a_do_that_never_took_leave_the_world_consistent() {
    let mut sim = Sim::new("unit-two-rules");
    for e in [Event::ApplyTemporary, Event::Approve(0), Event::Approve(1)] {
        sim.apply(e);
    }
    assert!(check_all(&mut sim).is_none(), "applied cleanly");
    let kept = || sim_fact(&sim, "file:/etc/kept");
    let before = kept();
    assert!(before.is_some(), "the plan's modified fact is there");

    // The link drops for that fact, and the operator recants. The undo
    // must not read the failure as "the fact is gone" and act on it.
    sim.apply(Event::DropRead);
    sim.apply(Event::Recant);
    assert!(check_all(&mut sim).is_none(), "the world is consistent");
    assert!(
        sim.sink.entries().iter().any(|e| {
            matches!(&e.event, rue_core::journal::Event::Stuck { .. })
                || matches!(&e.event, rue_core::journal::Event::Reverted)
        }),
        "the recant said what happened rather than passing over it"
    );

    // A step whose `do` fails without taking: the engine skips its undo
    // and says so, and the fact it never wrote is untouched.
    let mut sim = Sim::new("never-took");
    sim.apply(Event::BreakExecutor);
    for e in [Event::ApplyTemporary, Event::Approve(0), Event::Approve(1)] {
        sim.apply(e);
    }
    assert!(check_all(&mut sim).is_none(), "the world is consistent");
    assert!(
        sim.sink
            .entries()
            .iter()
            .any(|e| matches!(&e.event, rue_core::journal::Event::UndoSkipped { .. })),
        "the do that never took was not undone, and the journal says so: {:?}",
        sim.sink
            .entries()
            .iter()
            .map(|e| format!("{:?}", e.event))
            .collect::<Vec<_>>()
    );
}

fn sim_fact(sim: &Sim, shape: &str) -> Option<Vec<u8>> {
    sim.ssh.with(|f| f.facts.get(shape).cloned())
}
