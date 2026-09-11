# 0011: A step interrupted by the engine's death is undone even if it never took

- status: open
- kind: not-proven
- phase: 5
- opened: 2026-09-11

A failed step is undone only when its `do` changed an observed fact
(Phase 5's second unit), because an undo by name removes whatever holds
that name. A step whose `do` was in flight when the engine died is still
undone on the way back regardless: what its facts read before `do` was in
the memory that died (`engine/src/lifecycle.rs`, boot recovery).

**To close.** Persist the pre-`do` digests with the write-ahead entry and
apply the same rule at boot. That is a store-schema change (schema 3, and
`rued migrate`), which the second unit did not take.
