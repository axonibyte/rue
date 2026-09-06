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

## Status: Phase 0 built; its exit is the owner's

The Phase 0 prototype under `proto/` checks plans. The four acceptance
tenants of the roadmap's section 8 are encoded as terms and as unparsed
`.rue` text under `tenants/`, and their verdicts -- structured JSON against
`docs/verdict-schema.json`, the one-sentence prose, and the `explain`
listing -- are goldens the test suite compares byte for byte. Thirty
negative cases refuse with exactly their named code, one or more for every
code the prototype can raise. The twelve seeded rediscovery rows all
rediscover. The falsification sweep is in [`docs/prior-art.md`](docs/prior-art.md);
what the prototype learned, and the positions it took where section 5 was
silent, are in [`proto/README.md`](proto/README.md) for the owner to fold
back into the roadmap. Phase 0 exits when that is done and `reaper test` is
green on both registered guests; neither has happened yet.

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
| Verdicts on plans the tenants do not exercise | The checker is exercised by four tenants (seven host cases) and thirty negatives; nothing is proven about a construct none of them uses |
| The `.rue` text | Unparsed in Phase 0. The checked form of each tenant is its Haskell term; Phase 2's front end must accept the text and produce the same verdict |
| 26 of the 56 diagnostic codes | Emitted, each with a negative golden: E0201-E0203, E0205, E0207, E0208, E0301-E0305, E0401, E0403-E0405, E0407, E0410, E0501-E0509 |
| The other 30 codes | Not modeled: the surface's E01xx (parsing, names, kinds, totality), the secret rules E0206, E0209-E0211, E0411, the engine-shaped E0204, E0402, E0406, E0408, E0409, and the binding rules E0601-E0606. E0406 cannot arise in this model at all: installation precedes the first covered step by construction |
| Where section 5 was silent | The prototype took a position and recorded it in `proto/README.md` as a finding for the owner: thirteen items, from the requester as an input to `check` to the verdict's new `mode` field. None is folded into the roadmap yet |
| Windows beyond wine | The whole suite is built for `x86_64-pc-windows-gnu` and run under wine, on the Ubuntu reaper guest and in the pipeline; that proves the logic and the bytes and nothing about services, named pipes or the Task Scheduler, which Phase 3 tests on a real machine |
| The Rust pipeline steps | Present and gated: each prints a skip line until a `Cargo.toml` exists in Phase 1 |
| Deploy | Refuses on every tag until Phase 1 produces artifacts and a workspace version |

## Running the checks

```sh
sh tools/check.sh                 # the gate: every phase runs, every failure is reported
sh tools/lint-seam.sh             # the seam guard alone
sh tools/lint-ecodes.sh           # the E-code guard alone
sh tests/tier3/t_seam.sh          # a guard's self-test
sh tools/rediscovery/run.sh --tier 1   # revert each tier-1 protection in a scratch copy; the suite must fail
cargo build --workspace --all-targets --locked && cargo test --workspace --locked
cargo run -q -- check tenants/t1/expected/db-01/plan.json     # the prose verdict; --json, or `explain`, or `states`
cd proto && cabal build all && cabal test all --test-show-details=direct
cd proto && cabal run -v0 rue-proto-check -- t3 fw-01            # a tenant's prose verdict; --json, --explain
```

The crates need Rust 1.97 (`pkg install rust` on FreeBSD; the gate refuses
another minor). The prototype needs GHC 9.10.3 and cabal (`pkg install ghc
hs-cabal-install`). `proto/cabal.project` pins the Hackage index state and
`proto/cabal.project.freeze` pins every dependency, so the same inputs give
the same bytes on the workstation, in CI and in a reaper session.
`RUE_UPDATE_GOLDENS=1 cabal run rue-proto-goldens` is the only thing that
writes an expected file; the test suite is read-only and the gate fails if
a test run changes anything under `tenants/` or `docs/`.

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
| `tenants/harness/` | `rue-tenants`: the case table as code and the tests that hold `rue-core` to every Phase 0 golden byte for byte |
| `proto/` | The Phase 0 prototype (Haskell): library, tenants sublibrary, executables, tests; `proto/README.md` has the layout and the findings. The specification the crates are held to |
| `tenants/` | The acceptance tenants' `.rue` text, inventories and expected verdicts (Phase 0) |
| `tests/tier3/` | Self-tests of the guards: each plants the fault it exists to catch |
| `ci/build-target.sh` | All per-target build knowledge for the five release triples |
| `ci/image-digest.sh` | Resolves the GHC image digest `.reaper.toml` carries |
| `.reaper.toml` | reaper tenancy: two guests, two execution modes |
| `bitbucket-pipelines.yml` | Mirror first; the gate; gated Rust builds; tag-only deploy |

## License

BSD-2-Clause. Copyright (c) 2026 Axonibyte Innovations, LLC.
