# 0004: The complete simulation

- status: open
- kind: feature
- phase: 5
- opened: 2026-09-11

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
