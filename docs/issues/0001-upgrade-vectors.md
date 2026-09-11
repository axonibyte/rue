# 0001: Upgrade vectors: an older release's texts and store check and migrate, or say what changed

- status: closed
- kind: feature
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-11

A previous release's `.rue` files and store must still check and migrate
under the current one, or fail with a diagnostic that names the change and
its fix (ROADMAP Phase 5, deliverables and acceptance).

**Why now.** It is already true that v0.1.0's T2 text no longer checks: it
is refused with E0608 (Phase 4) and E0609 (Phase 5's second unit), neither
of which says "this is a change since v0.1.0, and here is the migration".
Every release that ships without this makes it harder to add.

**Done when.** The v0.1.0 and v0.2.0 tenant texts and a store written by
each are checked-in fixtures; a test checks each text under the current
build and requires a clean check or diagnostics that name the release and
the change; `rued migrate` takes each store forward; the acceptance line
("v0.1.0 tenant files under v0.3.0 pass or emit a migration diagnostic
naming the change") holds.

**Closed.** Built. Every code added since v0.1.0 (E0607, E0608, E0609) carries the release it came in and a migration, appended to its message (Code::since, Code::migration). tenants/_upgrade/ holds the tenant texts v0.1.0 and v0.2.0 shipped, byte for byte from their tags; tenants/harness/tests/upgrade.rs checks each of that release's cases under this build and requires every refusal to come from a newer rule that says so. engine/tests/fixtures/store-v0.2.0 is the store v0.2.0's own engine wrote, beside v0.1.0's; engine/tests/upgrade.rs migrates both to schema 3, drives their instances to a close, and verifies each release's journal and this build's entries as one chain. Rows migration-note-missing and older-record-unreadable.
