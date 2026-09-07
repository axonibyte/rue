# rue

**A language for provably reversible operations.**

Undo as a compiler verdict. A rue plan is a sequence of operations whose
reversibility -- how far it can be undone, from where, by whom, and past which
point it cannot -- is decided by `rue check` before anything runs, and stated
in one sentence.

The honesty caveat, on the first screen where it belongs: the checker's
theorem is *given honest declarations, the verdict is correct*. Rue reasons
over the footprints, undo loci and refusal modes an op declares. The engine
enforces those declarations at runtime by diffing observed facts against
them, but a probe that lies about the world, or a hook that lies about what
it did, is outside what any checker can see. Probe declarations, hook
implementations and the site's operator and registrar declarations are the
reviewable surface.

The plan of record is [`docs/ROADMAP.md`](docs/ROADMAP.md). Every rule is
stated once, in the section that governs it.

## Status: Phase 1 in progress; v0.0.1 tagged

The Rust workspace checks plans: `rue-core` is the checker, `rue` the
command line (`check`, `explain`, `states`) over a plan IR, `rue-tenants`
the harness. The four acceptance tenants of the roadmap's section 8 are
Rust terms under `tenants/harness/src/tenants/`, transcribed body by body
from the unparsed `.rue` text beside their goldens under `tenants/`, and
their verdicts -- structured JSON against `docs/verdict-schema.json`, the
one-sentence prose, and the `explain` listing -- are goldens the test suite
compares byte for byte. Thirty-six negative cases refuse with exactly their
named code, one or more for every code the checker can raise. Forty-three
rediscovery rows all rediscover. Bodies, closure analysis, secret placement,
the journal model with its canonical hash encoding, the request digests,
the injected `now`, the backstop artifact in `sh`, PowerShell and Python
(`rue-render`, `rue artifact`, the `sh` and Python artifacts executed in
tests) and the seeded fuzz properties are in; the Phase 1 acceptance is
met as far as a workstation and a pipeline can prove it, and
[`docs/DESIGN.md`](docs/DESIGN.md) says how the crates fit. Phase 2 has
begun: `rue-surface` parses every `.rue` text under `tenants/` into a
lossless tree and `rue fmt` is the identity on each; the resolver that
turns a text into the checker's input is next, and
[`docs/LANGUAGE.md`](docs/LANGUAGE.md) is the reader's guide as far as
the front end goes. The Phase 0 prototype under `proto/` is the record of what the
tenants taught ([`proto/README.md`](proto/README.md)); its own tests still
run in the gate. The falsification sweep is in
[`docs/prior-art.md`](docs/prior-art.md).

## What rue is not

Not a configuration-management or convergence tool; not a scheduler or
workflow engine; not a secrets manager; not an inventory system; not a
state-machine language; not a replication, detection, fencing, authority or
elevation engine. Those are ops, probes and bindings a tenant declares. Rue
sequences operations and proves their reversal. Where convergence is the
right model, rue is the wrong tool.

## Prior art, and the delta from each

The claim is narrow on purpose: rue is the first language for operations
against hosts in which "this can be undone" is a compile-time verdict rather
than a comment, computed from declared footprints rather than by search over
a world model, and stating where the undo runs, past which step it cannot,
what that step costs and who must acknowledge it. The sweep of 2026-09-06
([`docs/prior-art.md`](docs/prior-art.md)) found two fields that decide
undoability offline and were not in this table, and narrowed the claim to
that wording; the roadmap's section 1.1 still carries the broader sentence
for the owner to reconcile.

| Prior art | What it has | What rue adds |
|---|---|---|
| Action reversibility in AI planning (Eiter, Erdem & Faber 2008; Morak, Chrpa, Faber & Fišer, KR 2020; Med et al. 2024, 2025) | Decides offline whether an action's effects can be undone, by search over a STRIPS-like domain; PSPACE-hard | Decides from declarations, not search; the verdict states locus, cost, acknowledgement, arming order and bound, none of which the planning model has |
| Compensation calculi (Bruni, Melgratti & Montanari, POPL 2005; Sagas calculi; compensating CSP) | Semantics and expressiveness of compensations; decidability with static compensations | No footprints, no check that a given program's compensations compose; rue is the checker the calculi lack |
| Sagas / compensating transactions (Garcia-Molina & Salem, 1987) | Sequenced steps with hand-written compensations | The compensations are typed, checked for composition, and their locus is known |
| Temporal / Cadence | Durable execution; saga pattern for compensation | Checks nothing about compensations; no undo that survives the engine's death; no point of no return |
| Junos `commit confirmed` | Apply, auto-revert unless confirmed, on one device | Generalized to any op with a target-standalone undo; the `reach` rule proves the arming order |
| Database migrations (up/down) | Paired inverses | Unchecked; rue verifies inverse composition and footprint disjointness |
| NixOS generations | Atomic rollback of OS config | Restorative undo for one footprint kind only; no ops, no ordering, no cost |
| Terraform / Kubernetes | Declarative convergence | Convergence, not reversal; no concept of an irreversible step |
| Janus, reversible computing | Language-level reversibility | No side effects on a world; no footprints, no locus |
| Lenses / bidirectional transformations | Checked inverses over data | Data, not operations against hosts |
| Ansible `when:` / `block`/`rescue` | Conditional steps, rescue blocks | No checker; conflicts discovered at runtime on the host |
| Ecto.Multi | Named steps, all-or-nothing within a transaction | Inside one database; no partial reversibility, no locus |
| Miniscript / Antelope authorities | Threshold-and-timelock policy with load-time satisfiability | Policy, not operations; the model rue borrows for gates |
| Metafont / Dhall | Total, deterministic languages | The totality ethic rue adopts; neither is about effects |

## What is NOT proven, stated plainly

| Claim | Status |
|---|---|
| Verdicts on plans the tenants do not exercise | The checker is exercised by four tenants (seven host cases) and thirty-five negatives; nothing is proven about a construct none of them uses |
| The `.rue` text | Parsed and formatted, not yet resolved: the checked form of each tenant is still its Rust term under `tenants/harness/src/tenants/`, transcribed from the text body by body; Phase 2's resolver must derive the same plan IR from the text, and a test will hold the two equal |
| 32 of the 56 diagnostic codes | Emitted, each with a negative golden: E0201-E0203, E0205-E0211, E0301-E0305, E0401, E0403-E0405, E0407, E0410, E0501-E0509, E0606; E0109 by the renderer on a value it cannot quote, unit-tested |
| The other 24 codes | Not modeled: the surface's E01xx but E0109 (parsing, names, kinds, totality), E0411 (sinks are not declared in the site model), the engine-shaped E0204, E0402, E0406, E0408, E0409, and the binding rules E0601-E0605. E0406 cannot arise in this model at all: installation precedes the first covered step by construction |
| The backstop artifact | Rendered in `sh`, PowerShell and Python, and the `sh` and Python ones executed against a temporary instance directory in every scenario the tests name; that Phase 3's engine writes that directory as `docs/DESIGN.md` states, and that a real scheduler runs the script, are Phase 3's to prove. PowerShell is executed nowhere: no gate host runs it. A non-file fact under a computed undo is undone as if intact; a fact read in a covered undo is not bakeable this unit |
| Python artifacts on a target | `uv` present, an interpreter cached, `uv run --offline` viable at fire time: arm-time preconditions Phase 3 checks. The tests prove the invocation with a cached interpreter on the gate hosts |
| macOS | As a host, one artifact golden (T3's `fw-mac-01`). As a controller, the two darwin binaries are cross-built from Linux with zig and no SDK, clippy-clean and packaged, executed and signed nowhere until a Mac exists |
| E0202's positions | Closure treats a plan parameter and a host-record field as bakeable (closed) and an earlier step's output as never closed, positions section 5.3 does not state; recorded for the owner |
| E0206 | Decided only as a structural re-run: a `reestablish` primitive equal to one of the op's `do` primitives. The full rule ("reachable from") needs an op reference bodies do not carry |
| E0211 | Decided for static hosts only; a `:controller` step and a host bound at runtime are not judged at check |
| Where section 5 was silent | The prototype took a position and recorded it in `proto/README.md`: thirteen items, from the requester as an input to `check` to the verdict's `mode` field; the Rust crates reproduce each. The roadmap carries the owner's answers where given |
| Windows beyond wine | The whole suite is built for `x86_64-pc-windows-gnu` and run under wine, on the Ubuntu reaper guest and in the pipeline; that proves the logic and the bytes and nothing about services, named pipes or the Task Scheduler, which Phase 3 tests on a real machine |
| The seven build targets | Built, clippy-clean per target, and packaged in the pipeline (`ci/build-target.sh`); only the Linux x86-64 and wine-run Windows binaries execute the suite there, the others are cross-built and unexecuted until Phase 3's real machines |
| Deploy | Uploads the packaged artifacts on a tag equal to the workspace version, and refuses otherwise; nothing about the artifacts beyond the suite that produced them |

## Running the checks

```sh
sh tools/check.sh                 # the gate: every phase runs, every failure is reported
sh tools/lint-seam.sh             # the seam guard alone
sh tools/lint-ecodes.sh           # the E-code guard alone
sh tests/tier3/t_seam.sh          # a guard's self-test
sh tools/lint-darwin-deps.sh      # the darwin dependency guard alone
sh tools/rediscovery/run.sh --tier 1   # revert each tier-1 protection in a scratch copy; the suite must fail
cargo build --workspace --all-targets --locked && cargo test --workspace --locked
RUE_FUZZ_STEPS=5000 cargo test --workspace --locked --test fuzz   # the seeded properties, longer
cargo run -q -- check tenants/t1/expected/db-01/plan.json     # the prose verdict; --json, or `explain`, or `states`
cargo run -q -- artifact tenants/t3/expected/fw-01/plan.json --instance i-1   # the backstop artifact for the owner
cargo run -q -- fmt tenants/t1/plan.rue                       # the canonical layout of a .rue file; --check to only compare
cd proto && cabal build all && cabal test all --test-show-details=direct   # the Phase 0 record's own tests
```

The crates need Rust 1.97 (`pkg install rust` on FreeBSD; the gate refuses
another minor) and, for the artifact execution tests, `sh` and `uv` with a
cached interpreter (`pkg install uv`; `uv python install 3.12` where no
system Python serves). The prototype needs GHC 9.10.3 and cabal (`pkg install ghc
hs-cabal-install`). `proto/cabal.project` pins the Hackage index state and
`proto/cabal.project.freeze` pins every dependency, so the same inputs give
the same bytes on the workstation, in CI and in a reaper session.
`RUE_UPDATE_GOLDENS=1 cargo run -p rue-tenants --bin rue-goldens` is the only
thing that writes an expected file; the test suites are read-only and the
gate fails if a test run changes anything under `tenants/` or `docs/`.

`tools/check.sh` exits 0 only if every phase ran and passed. A phase whose
tool is missing exits 77 and counts as a failure unless the caller named it
in `RUE_CHECK_SKIP_OK`; the FreeBSD reaper guest declares its skips that way
in `.reaper.toml`, and nothing is ever assumed about the host.

Under reaper: `reaper up`, then `reaper test` runs sync, build and the gate
on both registered guests. Validate the manifest with
`reaper-manifest-validate .reaper.toml` before you need it.

## What is here

| Path | What |
|---|---|
| `docs/ROADMAP.md` | The plan of record: claim, model, surface, engine, tenants, phases, tests |
| `docs/prior-art.md` | The falsification sweep of 2026-09-06: every candidate, what was checked, the delta or the narrowing |
| `docs/TESTING.md` | The gate's phases, the tiers, goldens, canonical JSON, negatives, mutation checks, rediscovery, and what green does not prove |
| `docs/verdict-schema.json` | The structured verdict's schema, version 1; every golden validates and every declared field is produced |
| `docs/state-transitions.tsv` | The runtime state machine's full transition table, generated from its five rules |
| `tools/check.sh` | The gate |
| `tools/lint-seam.sh`, `tools/seam-denylist.txt` | The seam guard and its denylist |
| `tools/lint-ecodes.sh` | The E-code guard: `Rue.Proto.Diagnostics` and the roadmap's table must agree |
| `tools/lint-goldens.sh` | Golden hygiene: no CR, no trailing whitespace, one trailing LF |
| `tools/rediscovery/` | The rediscovery battery: a table of protections, a patch reverting each, `run.sh` to prove the suite catches every reversion, `check-patches.sh` in the gate so no patch rots |
| `Cargo.toml`, `core/` | The Rust workspace and `rue-core` (Phase 1): the model and its plan-IR shape, the checker, the verdict and its prose, `explain`, the state machine, the ledger; pure, no I/O |
| `cli/` | `rue`, the operator CLI: `check`, `explain` and `states` over a plan IR document, exit codes per the roadmap's section 6.8 |
| `tenants/harness/` | `rue-tenants`: the tenants and negatives as Rust terms, the case table as code, the tests that hold `rue-core` to every golden byte for byte, and `rue-goldens`, the only writer |
| `proto/` | The Phase 0 prototype (Haskell), kept as the record of what Phase 0 proved: its library, its state-table printer and its tier-1 and tier-4 tests still build and run in the gate; the goldens are the Rust crates' now. `proto/README.md` has the layout and the findings |
| `tenants/` | The acceptance tenants' `.rue` text, inventories and expected verdicts (Phase 0) |
| `tests/tier3/` | Self-tests of the guards: each plants the fault it exists to catch |
| `ci/build-target.sh` | All per-target build knowledge for the five release triples |
| `ci/image-digest.sh` | Resolves the GHC image digest `.reaper.toml` carries |
| `.reaper.toml` | reaper tenancy: two guests, two execution modes |
| `bitbucket-pipelines.yml` | Mirror first; the gate; gated Rust builds; tag-only deploy |

## License

BSD-2-Clause. Copyright (c) 2026 Axonibyte Innovations, LLC.
