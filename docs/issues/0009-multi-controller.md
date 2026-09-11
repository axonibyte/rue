# 0009: Multi-controller: refuse, or a lock protocol over a shared fact?

- status: open
- kind: question
- phase: 5
- opened: 2026-09-11

ROADMAP 11 leaves this for Phase 5: v0 is single-controller per host.
Two controllers acting on one host can today only be kept apart by
convention. Either refuse it, and document the refusal, or design a lock
protocol over a fact both can read.

**Recommendation.** Refuse and document it for v0.3.0; a lock protocol is
a design of its own, better taken when there is a tenant that needs it.
