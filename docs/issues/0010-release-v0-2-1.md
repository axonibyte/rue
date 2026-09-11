# 0010: Release v0.2.1 with the SDK fixes?

- status: closed
- kind: question
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-11

Phase 5's first unit fixed nineteen SDK defects, and the docs unit two
more (the Rust SDK's and the shim's serve loops); none is released. The
roadmap allows them out as v0.2.1 ahead of v0.3.0. main also carries the
second unit (IR 5, R0205, E0609), which a v0.2.1 tag from main would ship
too. Tags are the owner's to cut.

**Closed.** Decided by the owner's direction of 2026-09-11 ("move forward with your other questions how you see fit"): no v0.2.1. main already carries the second unit's language change (IR 5), so a v0.2.1 tagged from it would not be the SDK fixes alone; v0.3.0 carries them at Phase 5's exit.
