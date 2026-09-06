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

## Status: pre-Phase 0, scaffold in place

Nothing that checks a plan exists yet. This commit is the repository's shape:
the gate, the seam guard, the reaper tenancy, the pipeline, and the roadmap.
Phase 0 -- a Haskell prototype of the core algebra fed the four acceptance
tenants -- follows in this same unit of work.

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
| Any verdict about any plan | Nothing checks a plan yet. Phase 0 is the first attempt |
| The gate on a Windows guest | No reaper Windows template is registered yet; it is a Phase 1 deliverable. The manifest names the two registered guests and says so |
| The Rust pipeline steps | Present and gated: each prints a skip line until a `Cargo.toml` exists in Phase 1 |
| Deploy | Refuses on every tag until Phase 1 produces artifacts and a workspace version |

## Running the checks

```sh
sh tools/check.sh                 # the gate: every phase runs, every failure is reported
sh tools/lint-seam.sh             # the seam guard alone
sh tests/tier3/t_seam.sh          # the guard's self-test
```

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
| `tools/check.sh` | The gate |
| `tools/lint-seam.sh`, `tools/seam-denylist.txt` | The seam guard and its denylist |
| `tools/lint-goldens.sh` | Golden hygiene (wired into the gate once goldens exist) |
| `tests/tier3/` | Self-tests of the guards: each plants the fault it exists to catch |
| `ci/build-target.sh` | All per-target build knowledge for the five release triples |
| `ci/image-digest.sh` | Resolves the GHC image digest `.reaper.toml` carries |
| `.reaper.toml` | reaper tenancy: two guests, two execution modes |
| `bitbucket-pipelines.yml` | Mirror first; the gate; gated Rust builds; tag-only deploy |

## License

BSD-2-Clause. Copyright (c) 2026 Axonibyte Innovations, LLC.
