# Testing

How rue is tested in Phase 0, what the tests are allowed to do, and what the
suite being green does and does not prove. This document is binding on the
prototype the way reaper's `docs/testing-methodology.md` is binding on every
reaper tenant, and rue is one: never weaken a check to make a run pass; every
fix ships with the test that would have caught it; every new assertion is
mutation-checked before it counts; a pre-existing failure is proven by stash,
not assumed; a skipped phase says so on stderr and fails unless it was
declared skippable by the caller.

## The gate

`sh tools/check.sh` is the one command. It runs every phase, reports every
failure, and exits 0 only if every phase ran and passed:

| Phase | What it proves |
|---|---|
| `sh-syntax`, `bash-syntax` | Every shell file parses under the shell its shebang names |
| `shellcheck` | Every shell file is clean under shellcheck at its shebang's dialect |
| `seam` | No tenant or platform word (`tools/seam-denylist.txt`) appears outside `tenants/` |
| `ecodes` | `Rue.Proto.Diagnostics` and the roadmap's section 6.7 table name the same codes, and no `"E0xxx"` literal exists elsewhere |
| `golden-hygiene` | Every expected file has no CR, no trailing whitespace, exactly one trailing LF; JSON begins with `{` |
| `rediscovery-patches` | Every row of the rediscovery table names a patch that still applies to the tree, and every patch is listed |
| `tier3-selftests` | Each shell guard catches the fault it exists to catch, in a temporary tree |
| `cabal-build` | The prototype builds under GHC 9.10.3 with `-Wall -Werror` |
| `cabal-test` | The suite passes, and changed nothing under `tenants/` or `docs/` |

A phase whose tool is absent exits 77. That is a failure unless the caller
named the phase in `RUE_CHECK_SKIP_OK`. The FreeBSD reaper guest, which has
only the base system, declares `bash-syntax,shellcheck,cabal-build,cabal-test`
in `.reaper.toml`; nowhere else is anything skipped.

## Tiers in Phase 0

The roadmap's section 10.1 names seven tiers. Phase 0 has the four that run
on a workstation, all inside one tasty suite so a single `cabal test` is the
whole run:

| Tier | Group | What |
|---|---|---|
| 1 | `Test.Canonical`, `Test.Diagnostics`, `Test.Laws`, `Test.Check` | The canonical encoder's bytes and round trip; the code enumeration; the reversal laws as QuickCheck properties; every emitted code raised by one plan and not by its sibling |
| 2 | `Test.Golden`, `Test.Tenants` | Every artifact byte-identical to its expected file, no orphans; every tenant clean and every negative refused with exactly its code; the section 8 claims as verdict fields |
| 3 | `Test.Schema` (plus the shell guards in the gate) | Every verdict golden validates against `docs/verdict-schema.json`; every declared property path is produced by some golden |
| 4 | `Test.States`, `Test.Ledger` | The five state-machine rules over the generated table; the cross-plan ledger's reservations |

## Goldens

The list of goldens is `Rue.Proto.Tenants.artifacts`, a Haskell value, never
a directory listing. A missing expected file fails; an expected file no
artifact claims fails ("orphan"). The suite is read-only: the only writer is
`RUE_UPDATE_GOLDENS=1 cabal run rue-proto-goldens`, which refuses without the
variable, and the gate checksums `tenants/` and `docs/` before and after
`cabal test` and fails on any change. A mismatch prints the first differing
line with context and writes the actual bytes under
`$RUE_BUILDDIR/golden-actual/<path>` for diffing.

Regenerating goldens is a decision, not a fix. Read the diff. If the change
is intended, the commit body says why the verdict changed.

## Canonical JSON

The structured verdict is written in a canonical form so that a golden's
bytes are the verdict's meaning and nothing else, and so that Phase 1's Rust
implementation can reproduce it byte for byte. The form is specified here
and implemented once, in `Rue.Proto.Json.Canonical`:

- UTF-8. Object keys sorted by Unicode code point. Two-space indentation.
- `"key": value` with one space after the colon. Every array or object
  element on its own line, a comma after every element but the last. Empty
  containers are `[]` and `{}`.
- Numbers are integers only. A non-integer is a hard error, so no golden
  can depend on a float format.
- Strings escape `"`, `\`, and controls below U+0020: `\n \r \t \b \f` by
  name, anything else as `\u00xx` in lowercase hex. Everything else,
  including non-ASCII, is raw UTF-8.
- One trailing LF. No trailing whitespace anywhere.

This is byte-compatible with `serde_json::to_string_pretty` followed by a
newline. `Test.Canonical` asserts the bytes directly and, as a property,
that `parse . encode = id` and `encode . parse . encode = encode`.

## Negative cases

A negative case is a plan the checker must refuse with exactly one named
code. `Test.Tenants` requires the set of codes across the negative goldens to
equal `Test.Check.emittedCodes`, in both directions: a new check without a
negative golden fails, and a negative golden for a code the checker cannot
raise fails. The twelve of the roadmap's Phase 0 task 8 derive from T1 and
T3 by one change each; the rest are minimal plans on the lab site described
in `Rue.Proto.Tenants.Negative`.

## Mutation checks

Every new assertion is checked by breaking the thing it guards and watching
the suite fail, then restoring. A mutant that does not compile is no
evidence and is redone in a form that does. The commit body names the
mutants. The rediscovery table makes the most important of these permanent.

## Rediscovery

`tools/rediscovery/table.tsv` has one row per protection the project has
paid for: a patch under `tools/rediscovery/patches/` that reverts it, the
tier, and the tasty selector that must then fail.
`sh tools/rediscovery/run.sh --tier N` copies the tree to a scratch
directory per row, runs the selector there (it must pass and select at
least one test), applies the patch without fuzz, requires the patched tree
to compile, and requires the selector to fail with "tests failed". It prints
`N rediscovered, M not` and exits 0 only when M is 0. Run it before a
milestone is trusted; it takes a full build per row and nothing runs it
automatically. The gate's `rediscovery-patches` phase is the cheap half:
every patch must still apply, so a refactor that moves a protected check is
caught at once rather than when someone remembers the battery.

## Under reaper

`.reaper.toml` runs the whole gate in a digest-pinned GHC 9.10.3 image on the
Ubuntu guest, and the POSIX-sh half on the FreeBSD host guest with its skips
declared. `reaper up && reaper test`. The manifest validates with
`reaper-manifest-validate .reaper.toml`.

## What green does not prove

- Nothing about a construct no tenant or negative case uses.
- Nothing about the `.rue` text: it is unparsed in Phase 0, and only its
  existence per case is asserted.
- Nothing about hosts: no executor, no backstop artifact, no engine exists.
  Tiers 5 to 7 begin in Phase 3.
- Nothing on Windows: no reaper Windows guest is registered until Phase 1.
