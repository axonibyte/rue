# Upgrade vectors

The tenant texts as each release shipped them, byte for byte from its tag
(`git archive vX.Y.Z tenants`): each tenant's `plan.rue` and the
`inventory.toml` it reads. They are not today's tenants and are never
edited; a release that changes a rule adds its own directory beside them.

`tenants/harness/tests/upgrade.rs` checks every one of them, for every host
and plan, under the current build, and requires each either to check clean
or to be refused only by codes added after its release -- codes that say
so, with the change to make (`Code::since`, `Code::migration`). That is
Phase 5's acceptance line: "v0.1.0 tenant files under v0.3.0 pass or emit a
migration diagnostic naming the change."

The stores each release wrote are `engine/tests/fixtures/store-<release>`,
written by that release's own engine; `engine/tests/upgrade.rs` migrates
each to the current schema and drives its instance.
