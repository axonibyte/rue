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
| `cargo-fmt` | The workspace is rustfmt-clean |
| `cargo-clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` is clean |
| `cargo-build` | The workspace builds, every target, release, against the lockfile, under Rust 1.97 |
| `cargo-test` | The Rust suite passes, and changed nothing under `tenants/` or `docs/` |
| `cabal-build` | The prototype builds under GHC 9.10.3 with `-Wall -Werror` |
| `cabal-test` | The Haskell suite passes, and changed nothing under `tenants/` or `docs/` |

A phase whose tool is absent exits 77. That is a failure unless the caller
named the phase in `RUE_CHECK_SKIP_OK`. The FreeBSD reaper guest, which has
only the base system, declares the bash, shellcheck, cabal and cargo phases in
`.reaper.toml`; nowhere else is anything skipped. A Rust toolchain that is
present but not 1.97 is a failure, not a skip: the workspace's `rust-version`,
the CI image and the gate say one minor so fmt and clippy output is
comparable everywhere.

Two implementations, one set of goldens: the Haskell prototype writes them
and its suite compares them; the Rust crates compare the same files. Both run
in one gate, so the two cannot disagree while the gate is green.

## Tiers in Phase 0

The roadmap's section 10.1 names seven tiers. Phase 0 has the four that run
on a workstation, all inside one tasty suite so a single `cabal test` is the
whole run:

| Tier | Group | What |
|---|---|---|
| 1 | Rust `core/tests/{canonical,canon,diagnostics,ir,laws,interference,gates,intent_backstop,check,render,journal,request}.rs`; Haskell `Test.Canonical`, `Test.Diagnostics`, `Test.Laws`, `Test.Check` | The canonical encoder's bytes and round trip; the hash encoding's bytes; the code enumeration; the IR spelling; the reversal laws as properties; the interference rules one by one; every emitted code raised by one plan and not by its sibling; the prose and explain clauses; the journal chain and the digests |
| 2 | Rust `tenants/harness/tests/{goldens,tenants}.rs` | Every artifact byte-identical to its expected file, no orphans and none missing; the terms and the case table 1:1; every tenant clean and every negative refused with exactly its code; the section 8 claims as verdict fields |
| 3 | Rust `tenants/harness/tests/schema.rs` (plus the shell guards in the gate) | Every verdict validates against `docs/verdict-schema.json`; every declared property path is produced by some verdict |
| 4 | Rust `core/tests/{states,ledger}.rs`; Haskell `Test.States`, `Test.Ledger` | The five state-machine rules over the generated table; the cross-plan ledger's reservations; expiry and renewal against an injected now |

`tenants/harness` (`rue-tenants`) holds the tenants and the negatives as
Rust terms, the case table as code (`TENANT_CASES`, `NEGATIVES`,
`EMITTED_CODES`), the golden plumbing, and the writer; it lives under
`tenants/` because it names tenants. The Haskell prototype is the Phase 0
record: its tier-1 and tier-4 tests still run in the gate, but it no longer
compares against or writes the goldens.

## Goldens

The list of goldens is `rue_tenants::artifacts()`, computed from the terms
under `tenants/harness/src/tenants/`, never from a directory listing. A
missing expected file fails; an expected file no artifact claims fails
("orphan"). The suite is read-only: the only writer is
`RUE_UPDATE_GOLDENS=1 cargo run -p rue-tenants --bin rue-goldens`, which
refuses without the variable (a test proves it refuses and touches nothing),
and the gate checksums `tenants/` and `docs/` before and after both test runs
and fails on any change. A mismatch prints the first differing line with
context and writes the actual bytes under `target/golden-actual/<path>` for
diffing.

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

## The plan IR

The checker's input as data: one `plan.json` per checked case beside its
verdict goldens (`tenants/<t>/expected/<host>/plan.json`,
`tenants/_negative/<code>-<slug>/expected/plan.json`), holding what `check`
consumes -- the site, the requester and one concrete per-host plan -- in
canonical JSON. It is a golden like the others: produced from the case's
term by `rue-goldens`, read only in tests (which also parse it back and
require the term), covered by the hygiene guard and the orphan walk.

The shape is `rue_core::model`'s serde form, spelled deliberately field by
field so no implementation's constructor names leak into it. `ir_version` is
an integer, currently 2; a reader refuses any version it does not know. While
the terms are the only emitter, any change of shape bumps the version and
changes emitter and readers in one commit; Phase 2's front end freezes it.
Durations are whole seconds under names ending in `_s`. A unit constructor is
a bare string, a data-carrying one a one-key object, and every item carries
an `item` tag with a step's fields flattened beside it.

Version 2 carries bodies (`rue_core::body`). An op has `do`, an `undo` that
is `"restore"`, `"none"`, or `{"computed": {"body", "undo_pre"}}` /
`{"compensate": {...}}`, and `suspend` and `reestablish` bodies or `null`. A
body is a list of one-key primitive objects (`run`, `write`, `remove`,
`append`, `region_set`, `region_clear`, `stage`, `hook`, `install`,
`release`, `call`); a value is `{"lit"}`, `{"ref"}` or `{"template": [parts]}`,
and a reference names its origin (`fact`, `param`, `host`, `output`,
`controller`, `secret`), which is what closure (E0202) and secret placement
read. Nothing in the IR says "closed" or "secret" as a flag: both are
computed from the structure. A host record carries `stdin_preamble` and the
site `secrets_deliver_to`. `undo_idempotent` remains a stand-in until E0208's
analysis exists. `core/tests/ir.rs` holds a document exercising every
primitive and asserts it reads and writes back byte for byte.

`rue check <plan.json>` reads one.

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
tier, the suite the selector runs in (`cabal` for the prototype, `cargo` for
the workspace), and the selector that must then fail (a tasty `-p` pattern or
a cargo test-name filter). A protection the Rust crates carry has a `-core`
row of its own beside the Haskell one, since each is a separate check that
can rot separately.
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

`.reaper.toml` runs the whole gate on the Ubuntu guest, in the digest-pinned
Rust 1.97 image the pipeline uses (Debian trixie) with GHC 9.10.3 and cabal
installed by ghcup into the guest's caches on first use, and the POSIX-sh half
on the FreeBSD host guest with its skips declared. `reaper up && reaper test`. The manifest validates with
`reaper-manifest-validate .reaper.toml`.

Windows is tested under wine: `ci/test-windows.sh` builds the whole suite for
`x86_64-pc-windows-gnu`, statically linked against the C runtime
(`.cargo/config.toml`) so the binaries carry no mingw DLL dependency, and
runs it with wine as cargo's runner, on the Ubuntu reaper guest after the gate
and in the pipeline's `doTestWindows` step. Rust's standard library needs
`bcryptprimitives.dll`, which wine has had since 8.13; Debian trixie's wine 10
qualifies and bookworm's 8.0 does not, which is why both hosts are trixie. That
proves the crates' logic and the CLI's bytes on the Windows target. What wine
cannot exercise -- services, named pipes, the Task Scheduler, ACLs -- is
Phase 3's to test on a real machine.

## What green does not prove

- Nothing about a construct no tenant or negative case uses.
- Nothing about the `.rue` text: it is unparsed in Phase 0, and only its
  existence per case is asserted.
- Nothing about hosts: no executor, no backstop artifact, no engine exists.
  Tiers 5 to 7 begin in Phase 3.
- On Windows, only what wine can show: the suite passing on the windows-gnu
  target. Services, named pipes, the Task Scheduler and ACLs are Phase 3's,
  on a real machine.
