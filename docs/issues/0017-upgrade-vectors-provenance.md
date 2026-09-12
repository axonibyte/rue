# 0017: The upgrade vectors assert their provenance and nothing checks it

- status: open
- kind: defect
- phase: 5
- opened: 2026-09-12

`tenants/_upgrade/README.md` says the vectors are the tenant texts "byte for
byte from its tag (`git archive vX.Y.Z tenants`)", and
`engine/tests/fixtures/store-<release>` the store each release's own engine
wrote. Both claims are the foundation of Phase 5's acceptance line -- v0.1.0
tenant files must check under v0.3.0 or be refused only by codes added since.

**Nothing verifies either claim.** No test compares a vector against the tag
it names. A vector edited by hand, or copied from a working tree rather than
an archive, would pass every suite and quietly turn the upgrade test into a
test of today's text against today's build -- the exact failure this project
keeps finding elsewhere: the thing being checked and the thing doing the
checking come from the same place.

Checked by hand today, and it currently holds: all 8 files of `v0.1.0` and
all 8 of `v0.2.0` are byte-identical to `git archive <tag> tenants`, with
the tags at f247771 and e08fe1e. So this is a missing guard, not a live
defect.

**Where it came from.** The coop room spent an hour citing a bonemesh
checkout that was three and a half years stale while being faithfully
current with the branch it tracked; the rule wren drew out of it is *cite
refs, not repositories* -- `<host>/<repo>@<ref>=<commit>`, dated, because
"the repository" does not have one state. rue makes the same shape of claim
about its own past and records only a tag name.

**Done when.** Each vector directory records the commit its tag resolved to
when it was made, and a test regenerates the vector from that commit and
compares byte for byte -- so a drifted vector fails rather than passes. The
same for the store fixtures, which additionally cannot be regenerated (they
were written by an engine that no longer exists), and whose honest record is
therefore the commit, the date, and the schema they were written at.
