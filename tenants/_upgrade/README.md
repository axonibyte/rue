# Upgrade vectors

The tenant texts as each release shipped them, byte for byte from its tag
(`git archive vX.Y.Z tenants`): each tenant's `plan.rue` and the
`inventory.toml` it reads. They are not today's tenants and are never
edited; a release that changes a rule adds its own directory beside them.

**And that is checked rather than asserted.** `PROVENANCE` beside this file
records the commit each tag resolved to when its vector was cut, and
`tools/lint-provenance.sh` (gate phase `provenance`) regenerates every file
from that commit and compares it byte for byte -- so a vector edited by hand,
or copied from a working tree rather than from the tag, fails the gate instead
of quietly turning the upgrade test into a test of today's text against
today's build. A directory added here without a section in `PROVENANCE` fails
too.

`tenants/harness/tests/upgrade.rs` checks every one of them, for every host
and plan, under the current build, and requires each either to check clean
or to be refused only by codes added after its release -- codes that say
so, with the change to make (`Code::since`, `Code::migration`). That is
Phase 5's acceptance line: "v0.1.0 tenant files under v0.3.0 pass or emit a
migration diagnostic naming the change."

The stores each release wrote are `engine/tests/fixtures/store-<release>`,
written by that release's own engine; `engine/tests/upgrade.rs` migrates
each to the current schema and drives its instance. They carry their own
`PROVENANCE` on different terms, because a store fixture cannot be
regenerated -- the engine that wrote it is gone -- and that file says exactly
what is checked and what is only asserted.
