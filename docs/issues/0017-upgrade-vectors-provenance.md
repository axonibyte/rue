# 0017: The upgrade vectors assert their provenance and nothing checks it

- status: closed
- kind: defect
- phase: 5
- opened: 2026-09-12
- closed: 2026-09-12

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

**Closed 2026-09-12.** Each set now carries a `PROVENANCE` record naming the
commit, and `tools/lint-provenance.sh` (gate phase `provenance`) checks it.

The vectors are regenerated from the commit their record names and compared
byte for byte, so a drifted vector fails rather than passes. A tag that no
longer resolves to the recorded commit is reported before any byte
comparison, because it explains every mismatch under it; a clone without the
tag is not a failure, since the commit is the record and the tag name is the
human's half of it. A directory beside a record with no section in it fails,
which is what makes an unrecorded vector loud instead of silent.

The store fixtures are checked on different terms and the record says so
rather than implying the two are equally provable. They cannot be
regenerated -- the engine that wrote them is gone from the tree -- so the
guard requires that every file is byte for byte what it was at the recorded
commit **and** that the commit is still the last one to touch the directory.
Together those are "unchanged since it was recorded": edit one in the tree
and the bytes differ; edit it and commit, and the last-touching commit moves.
What nothing in the tree can establish is that a fixture was written by the
release it names, because the evidence would be the engine that is gone;
`written_by` is marked in the record as an assertion instead of being dressed
as a check.

`tests/tier3/t_provenance.sh` builds scratch repositories with `git init` and
requires the guard to fail on each way a record can stop being true -- an
edited vector, a file the tag never had, a moved tag, a fixture edited in the
tree, a fixture rewritten and committed, a schema disagreeing with its
record, an unrecorded directory, an abbreviated commit -- and to exit 2,
never 0, where there is no history to read. Writing it found a defect in its
own fixtures first: a record naming the commit that adds a fixture cannot
live inside that commit, because amending it in moves the commit it names.
The record is its own commit, which is also how the repository's is arranged.

**What this cost elsewhere, stated because it is a new dependency and not
only a new file.** The guard reads the repository's history, so the gate now
needs `.git` and git itself. reaper syncs the tree with its history already;
`git` is declared in both guests' package lines, and the pipeline's gate step
clones at full depth (the recorded commits are older than the default depth
of 50) and installs git. No phase is declared skippable for it anywhere: where
the history cannot be read the guard exits 2 and fails, rather than passing
something it could not check.
