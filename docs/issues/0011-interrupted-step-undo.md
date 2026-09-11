# 0011: A step interrupted by the engine's death is undone even if it never took

- status: closed
- kind: not-proven
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-11

A failed step is undone only when its `do` changed an observed fact
(Phase 5's second unit), because an undo by name removes whatever holds
that name. A step whose `do` was in flight when the engine died is still
undone on the way back regardless: what its facts read before `do` was in
the memory that died (`engine/src/lifecycle.rs`, boot recovery).

**To close.** Persist the pre-`do` digests with the write-ahead entry and
apply the same rule at boot. That is a store-schema change (schema 3, and
`rued migrate`), which the second unit did not take.

**Closed.** Fixed. The pre-do digests are kept beside the write-ahead entry (InstanceRecord.attempting_pre, store schema 3, rued migrate 2 -> 3), and boot recovery applies the failed-step rule: a step in flight whose facts read as they did before do is not undone (UndoSkipped), one whose facts changed is, and a record that kept nothing -- written before schema 3 -- is undone as before. Tests in engine/tests/lifecycle.rs hold all three; row boot-undoes-a-step-that-never-took.
