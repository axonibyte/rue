# 0004: The complete simulation

- status: closed
- kind: feature
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-12

`sim/` is tier 7: seeded event lists against a real engine, the twenty
invariants of ROADMAP 10.3 after every event, and a shrinker. Its world is
two plans cut from T1 and T3, four of the invariants are unreachable in it
(docs/TESTING.md, "The simulation"), and it applies the artifact's rule to
the shadow rather than running the rendered script.

**Done when.** The world gains T2- and T4-shaped plans -- a repeat, a
deferred step and its handoff, hook executors, a non-file fact read
through a `reads` probe -- and the second unit's rules (R0205 reads,
`UndoSkipped`) are in the shadow; every invariant is reached by some
seed, or named with where it is proven instead.

**Closed.** Done, 2026-09-12, in two commits. The world gained a third plan cut from T2 and T4 -- a repeat over a list with a fact per iteration, a staged file, a region on the file the other two hold regions on, a `modified` fact that is not a file and is read by the probe declaring it `reads`, a step behind a step gate on an appliance with no filesystem, and a step no transport reaches that defers until `handoff-done` -- and the events it needs, including `DropRead` for unit 2's R0205 rule. Every invariant the world can reach now has a violation planted by hand and caught: eighteen of twenty, with 6 (no wane during settle) and 18 (no undeclared act) named with where each is proven instead, both structural to a sim that boots inside one call and never opens the control channel.

What the widening found, all of it in the shadow rather than the engine: a footprint shape naming a repeat's variable was looked for literally; a step performed out of band on a host no executor reaches was asserted as a fact; invariants 7 and 10 read one host's whole ordering as one instance's, so an uncovered plan's runs answered a covered plan's question; and invariant 12's planted violation had been skipping itself in silence for want of two live region holders, the permanent plan having committed in one apply. The fake executor gained an instance-tagged act log, and `request` now records a refusal in the sim's notes -- a refused plan was a silent no-op, and an event list full of those is a sweep that looks twice the size it is. Rows: `sim-reads-a-repeat-shape-literally`, `sim-judges-an-artifact-across-instances`, `sim-skips-a-plant-in-silence`.

Not proven: the artifact's rule is still applied to the shadow rather than the rendered script being executed, which `render/tests/execute.rs` and the e2e harness cover; and the sweep is 39 seeds of 24 events, which is a sample and not a proof.
