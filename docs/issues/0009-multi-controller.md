# 0009: Multi-controller: refuse, or a lock protocol over a shared fact?

- status: closed
- kind: question
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-11

ROADMAP 11 leaves this for Phase 5: v0 is single-controller per host.
Two controllers acting on one host can today only be kept apart by
convention. Either refuse it, and document the refusal, or design a lock
protocol over a fact both can read.

**Recommendation.** Refuse and document it for v0.3.0; a lock protocol is
a design of its own, better taken when there is a tenant that needs it.

**Closed.** Settled by refusing what a target can decide, 2026-09-11. A
store names its controller once (`<store>/controller`, sixteen random bytes
in hex) and stamps that id into every instance directory it creates. A host
carrying a directory stamped by another controller whose artifact is armed
and unfired holds that controller's live commitment -- its backstop will
undo work there on its own schedule -- and the apply is refused before
anything is created (R0409), naming the directory so an operator can read
it and `rue reclaim --force --reason` it if it is spent. Boot
reconciliation leaves every foreign directory exactly as it is
(`InstanceDirForeign`, reported by `rue doctor` apart from the orphans),
spent ones included: they are another controller's evidence, not this
store's to reclaim. A directory with no stamp predates the id and is read
as this controller's, so an upgrade refuses nothing it already holds.

The first draft refused on any foreign directory at all, and the e2e guests
refuted it within one cycle: every stage there is a store of its own, and a
fired directory a killed controller had left behind would have closed the
host to every later stage for good. What is dangerous is a live commitment,
not a spent one, and that is what the rule now says.

No protocol change was needed: the stamp is written with `put_file` and
read with `get_file`, both v1 ops. Two controllers acting at once with
nothing armed between them is what a lock protocol over a fact both can
read would decide; it remains undesigned and unbuilt, to be taken when a
tenant needs it, and ROADMAP 11 records the decision.
