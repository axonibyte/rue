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

## Status: Phase 0 in progress

The Phase 0 prototype under `proto/` checks plans. The four acceptance
tenants of the roadmap's section 8 are encoded as terms and as unparsed
`.rue` text under `tenants/`, and their verdicts -- structured JSON against
`docs/verdict-schema.json`, the one-sentence prose, and the `explain`
listing -- are goldens the test suite compares byte for byte. Thirty
negative cases refuse with exactly their named code, one or more for every
code the prototype can raise. The rediscovery table and the falsification
day's findings follow in this same unit of work.

## What rue is not

Not a configuration-management or convergence tool; not a scheduler or
workflow engine; not a secrets manager; not an inventory system; not a
state-machine language; not a replication, detection, fencing, authority or
elevation engine. Those are ops, probes and bindings a tenant declares. Rue
sequences operations and proves their reversal. Where convergence is the
right model, rue is the wrong tool.

## Prior art, and the delta from each

The claim is narrow on purpose: rue is the first plan language in which "this
can be undone" is a compile-time verdict rather than a comment. The table is
the falsification attempt; before Phase 0 exits, one person spends a day
trying to break it, and the findings go in [`docs/prior-art.md`](docs/prior-art.md).

| Prior art | What it has | What rue adds |
|---|---|---|
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
| Where section 5 was silent | The prototype took a position and recorded it as a finding for the owner (see `proto/README.md` when it lands): the requester is an input to `check`; the verdict holds per knell segment; a step is deferred when its host is unreachable by the site's transports or bound at runtime; `Pending` is bounded by the gate window or `max_wait` (E0506 otherwise); facts are scoped by host; par siblings have no order and are judged only by disjointness; a repeated anchor is E0305 alone |
| The gate on a Windows guest | No reaper Windows template is registered yet; it is a Phase 1 deliverable. The manifest names the two registered guests and says so |
| The Rust pipeline steps | Present and gated: each prints a skip line until a `Cargo.toml` exists in Phase 1 |
| Deploy | Refuses on every tag until Phase 1 produces artifacts and a workspace version |

## Running the checks

```sh
sh tools/check.sh                 # the gate: every phase runs, every failure is reported
sh tools/lint-seam.sh             # the seam guard alone
sh tools/lint-ecodes.sh           # the E-code guard alone
sh tests/tier3/t_seam.sh          # a guard's self-test
sh tools/rediscovery/run.sh --tier 1   # revert each tier-1 protection in a scratch copy; the suite must fail
cd proto && cabal build all && cabal test all --test-show-details=direct
cd proto && cabal run -v0 rue-proto-check -- t3 fw-01            # a tenant's prose verdict; --json, --explain
```

The prototype needs GHC 9.10.3 and cabal (`pkg install ghc hs-cabal-install`
on FreeBSD). `proto/cabal.project` pins the Hackage index state and
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
| `docs/prior-art.md` | The falsification day's findings (Phase 0) |
| `docs/verdict-schema.json` | The structured verdict's schema, version 1; every golden validates and every declared field is produced |
| `docs/state-transitions.tsv` | The runtime state machine's full transition table, generated from its five rules |
| `tools/check.sh` | The gate |
| `tools/lint-seam.sh`, `tools/seam-denylist.txt` | The seam guard and its denylist |
| `tools/lint-ecodes.sh` | The E-code guard: `Rue.Proto.Diagnostics` and the roadmap's table must agree |
| `tools/lint-goldens.sh` | Golden hygiene: no CR, no trailing whitespace, one trailing LF |
| `tools/rediscovery/` | The rediscovery battery: a table of protections, a patch reverting each, `run.sh` to prove the suite catches every reversion, `check-patches.sh` in the gate so no patch rots |
| `proto/` | The Phase 0 prototype (Haskell): library, tenants sublibrary, executables, tests |
| `tenants/` | The acceptance tenants' `.rue` text, inventories and expected verdicts (Phase 0) |
| `tests/tier3/` | Self-tests of the guards: each plants the fault it exists to catch |
| `ci/build-target.sh` | All per-target build knowledge for the five release triples |
| `ci/image-digest.sh` | Resolves the GHC image digest `.reaper.toml` carries |
| `.reaper.toml` | reaper tenancy: two guests, two execution modes |
| `bitbucket-pipelines.yml` | Mirror first; the gate; gated Rust builds; tag-only deploy |

## License

BSD-2-Clause. Copyright (c) 2026 Axonibyte Innovations, LLC.
