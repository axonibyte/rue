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

## Status: Phase 5 complete; v0.3.0 tagged

Rue has tools now, and drills. A plan is highlighted by a tree-sitter
grammar that parses every tenant text the front end accepts and refuses
every one it refuses, held to that by a guard that runs both parsers over
every `.rue` file in the repository. A language server answers an editor
with the checker's own diagnostics and `explain`'s own words -- never a
second opinion of its own -- and says which host it judged when a file's
clauses dispatch on one. `rue explain --html` renders a plan as a page that
fetches nothing, for a change record or an incident's notes.

`rue drill` applies a plan to a host the inventory declares a canary,
recants it, and leaves an attestation in the journal naming every fact it
touched with the digest before and after; `rue journal verify
--attestations` reads them back out of the verified chain. That is the
difference between "the undo is written" and "the undo ran last night and
put the machine back". Two refusals guard it, and a fact the engine could
not read attests to nothing and says so.

The dead man is proven under a real severed link. A plan with
`unless_heartbeat` is applied, the controller's path to the target is cut,
and the target's own cron fires the artifact on the stale beat and undoes
the step with no engine involved -- the engine alive and unreachable, which
is the case the trigger exists for and the one every earlier proof faked by
killing the engine. Two controllers on one host is answered too: a store
names its controller, stamps it into every instance directory it creates,
and refuses a host holding another controller's live backstop.

## About the name

Two other programming languages are already called Rue, and `rue` is taken
on crates.io, PyPI and npm; the sweep is in
[`docs/prior-art.md`](docs/prior-art.md), with what is free and what a
public rue would walk into. The name has not been decided, and nothing here
depends on the decision.

## Status: Phase 4

A host process can embed rue. It holds one connection to `rued` and is, on
that one connection, a declared operator issuing verbs, a registrar whose
hooks the engine calls back into, and a subscriber to its own plans. The
hook protocol is frozen at v1 (`docs/hook-protocol-v1.json`, pinned by
the gate), and it is spoken by six clients that agree on it case for
case: a Rust SDK, the `rue-hook` shim for shell tenants, and SDKs for
Python, Elixir, Java and .NET, each judged by `rue sdk-conform` against
one scripted world. A site's inventory can come from a hook, secrets
resolve through one, and an embedder gets the same verdict from the
daemon that a person gets from `rue check` at a terminal.

The two tenants that could only check now run. T4's reactive host, an
Elixir process, fires a temporary plan on entering a state and recants on
leaving it, over appliance facts that live in no filesystem, and both
drift policies hold or clobber a hand-flipped actuator. T2's cluster
succession runs over `jail(8)` on a FreeBSD guest: the corpse is fenced
through the cluster driver, the guests start as real jails, the heir on a
console-only node is handed off by a person, a knell's acknowledger is
shown the real list of what a ZFS rollback will destroy, a second promote
for the same corpse is refused, and a recant stops each guest it started.
Running T2 found what four phases of checking it had not; the roadmap's
Phase 4 entry lists what, and what it still does not prove.

## Status of Phase 3


Rue applies and reverts plans against real hosts. `rued` is a daemon over
a locked instance store with a chained, optionally signed journal; `rue`
is the whole verb list of the roadmap's 6.8 over a control channel whose
identity comes from the operating system and never from the client. Steps
run through `local()` and `ssh()` (or a hook), a step's footprint is
snapshotted before it runs and checked after, drift at undo time is
decided by the same rule the target-side artifact applies, and a
`:target` backstop is rendered, installed, armed through `cron()` and
fired by the target's own scheduler when the engine is not there. Gates
hold a plan at the door until enough proofs arrive; secrets are delivered
once, to the first acceptor that takes them, and never reach the store or
a journal entry.

What that is worth is what the end-to-end harness shows on a disposable
guest, both FreeBSD and Linux: a plan opens a port by a fenced region in
the host's packet filter and commits when confirmed; a hand edit behind
the engine's back holds the instance until it is forced; a write outside
a step's footprint is refused; a daemon killed inside a step's `do` comes
back and undoes it; a backstop armed by a daemon that then dies still
fires from the target's own cron; a recant racing a fired artifact
restores the file once, not twice; and `rue doctor --canary` proves a
real backstop fires by installing one and waiting for it. T1's
break-glass plan runs whole: two humans open the gate, an appliance
account is enabled through a hook, its credential is escrowed, and a
recant puts both hosts back.

Windows is built for `x86_64-pc-windows-gnu` and tested under wine, which
carries the named pipe end to end and names its client from that client's
own SID. What only a real Windows machine can show -- the service-control
manager, the kernel enforcing the pipe's list against a stranger, the
Task Scheduler, PowerShell as `local()`'s shell -- is Phase 3W's, named in
the roadmap.

## Status of the earlier phases

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
[`docs/DESIGN.md`](docs/DESIGN.md) says how the crates fit. Phase 2 is
complete as far as a workstation and a pipeline can prove it: `rue-surface`
parses every `.rue` text under `tenants/` into a lossless tree, `rue fmt`
is the identity on each, the resolver turns a text into the checker's
input for one host (`rue check file.rue --host H`), the texts are the
source of every golden, every code the front end raises has a diagnostic
golden, and the 200-step, 1,000-host check runs in tens of milliseconds
against a 2 s bound. [`docs/LANGUAGE.md`](docs/LANGUAGE.md) is the
language's guide, complete enough to write a tenant from. The Phase 0 prototype under `proto/` is the record of what the
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
| The `.rue` text | The source: every golden is derived from a text by the front end (`rue check tenants/t1/plan.rue`); the Rust terms that carried Phase 0's record retired once the front end reproduced each of them. What the grammar admits beyond the constructs the tenants use is proven by the parser corpus and the diagnostic goldens only |
| 51 of the 56 diagnostic codes | Each with a golden: 31 by the checker with a verdict golden (E0201-E0203, E0205-E0211, E0301-E0305, E0401, E0403-E0405, E0407, E0410, E0501-E0509, E0606); 19 by the front end with a diagnostics golden (E0101-E0108, E0110-E0114, E0204, E0601-E0605); E0109 by the renderer, unit-tested. A test holds the 56 to a partition of these lists and the next row |
| The other 5 codes | Not modeled, each with its reason in `rue_tenants::UNMODELED_CODES`: E0402 and E0408 are engine time, E0406 cannot arise (installation precedes the first covered step by construction), E0409 is fixed by `--host`, E0411 needs sinks the site does not declare |
| The backstop artifact | Rendered in `sh`, PowerShell and Python, and the `sh` and Python ones executed against a temporary instance directory in every scenario the tests name; that Phase 3's engine writes that directory as `docs/DESIGN.md` states, and that a real scheduler runs the script, are Phase 3's to prove. PowerShell is executed nowhere: no gate host runs it. A non-file fact under a computed undo is undone as if intact; a fact read in a covered undo is not bakeable this unit |
| Python artifacts on a target | `uv` present, an interpreter cached, `uv run --offline` viable at fire time: arm-time preconditions Phase 3 checks. The tests prove the invocation with a cached interpreter on the gate hosts |
| macOS | As a host, one artifact golden (T3's `fw-mac-01`). As a controller, the two darwin binaries are cross-built from Linux with zig and no SDK, clippy-clean and packaged, executed and signed nowhere until a Mac exists |
| E0202's positions | Closure treats a plan parameter and a host-record field as bakeable (closed) and an earlier step's output as never closed, positions section 5.3 does not state; recorded for the owner |
| E0206 | Decided only as a structural re-run: a `reestablish` primitive equal to one of the op's `do` primitives. The full rule ("reachable from") needs an op reference bodies do not carry |
| E0211 | Decided for static hosts only; a `:controller` step and a host bound at runtime are not judged at check |
| Where section 5 was silent | The prototype took a position and recorded it in `proto/README.md`: thirteen items, from the requester as an input to `check` to the verdict's `mode` field; the Rust crates reproduce each. The roadmap carries the owner's answers where given |
| Windows beyond wine | The whole suite is built for `x86_64-pc-windows-gnu` and run under wine, which does carry the named pipe end to end and name its client from that client's own SID; the service-control manager, the list as the kernel enforces it against a stranger, the Task Scheduler and PowerShell as `local()`'s shell are Phase 3W's, on a real machine |
| The simulation's reach | Tier 7 drives a real engine over seeded event lists and checks the twenty invariants of the roadmap's 10.3 after every event, but it applies the artifact's rule to its shadow rather than executing the rendered script. Two of the twenty are out of that world's reach and the test that says so names each and where it is proven instead; the other eighteen each have a violation planted by hand, so every check is known to fire. 39 seeds of 24 events is a sample, not a proof |
| `unless_heartbeat` under a partition | Proven on both guests under a severed link (`tenants/e2e/tests/partition.rs`): the cut is a firewall rule on the loopback path both ends share, not a vnet, so what is proven is that the engine is alive and unreachable and the target acts alone |
| Editors | The grammar parses every tenant text and its queries load; the language server answers the handlers' tests. No editor has painted either, and none is a test |
| Two controllers with nothing armed | A host holding another controller's *armed* backstop refuses the apply (R0409); two engines acting at once with nothing armed between them is undecided, and is the lock protocol the roadmap defers |
| A drill of a many-host or gated plan | `rue drill` admits both and neither suite exercises one |
| Schedulers other than cron | `task_scheduler()` and `launchd()` are written and unit-tested against a fake transport, and have installed nothing anywhere |
| The seven build targets | Built, clippy-clean per target, and packaged in the pipeline (`ci/build-target.sh`); only the Linux x86-64 and wine-run Windows binaries execute the suite there, the others are cross-built and unexecuted until Phase 3's real machines |
| Deploy | Uploads the packaged artifacts on a tag equal to the workspace version, and refuses otherwise; nothing about the artifacts beyond the suite that produced them |

## Running the checks

```sh
sh tools/check.sh                 # the gate: every phase runs, every failure is reported
sh tools/lint-seam.sh             # the seam guard alone
sh tools/lint-ecodes.sh           # the E-code guard alone
sh tools/lint-rcodes.sh           # every runtime code raised and tested
sh tests/tier3/t_seam.sh          # a guard's self-test
sh tools/lint-darwin-deps.sh      # the darwin dependency guard alone
sh tools/lint-cross-build.sh      # what compiles C is excluded from the cross-target builds
sh tools/lint-issues.sh           # the tracker and its index agree
sh tools/rediscovery/run.sh --tier 1   # revert each tier-1 protection in a scratch copy; the suite must fail
cargo build --workspace --all-targets --locked && cargo test --workspace --locked
RUE_FUZZ_STEPS=5000 cargo test --workspace --locked --test fuzz   # the seeded properties, longer
cargo run -q -- check tenants/t1/expected/db-01/plan.json     # the prose verdict; --json, or `explain`, or `states`
cargo run -q -- artifact tenants/t3/expected/fw-01/plan.json --instance i-1   # the backstop artifact for the owner
cargo run -q -- fmt tenants/t1/plan.rue                       # the canonical layout of a .rue file; --check to only compare
cargo run -q -- check tenants/t3/plan.rue --host fw-01 --json # a .rue text resolved for one host; --plan-name when the file defines several
cargo run -q -- explain tenants/t3/plan.rue --host fw-01 --html > plan.html  # the same listing as a page that fetches nothing
cargo run -q -- journal verify journal.ndjson --attestations  # the drills a chain carries, once the chain verifies
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
| `docs/issues/` | The issue tracker: a file per issue, and an index the gate keeps honest (`tools/lint-issues.sh`) |
| `docs/TESTING.md` | The gate's phases, the tiers, goldens, canonical JSON, negatives, mutation checks, rediscovery, and what green does not prove |
| `docs/verdict-schema.json` | The structured verdict's schema, version 1; every golden validates and every declared field is produced |
| `docs/state-transitions.tsv` | The runtime state machine's full transition table, generated from its five rules |
| `tools/check.sh` | The gate |
| `tools/lint-seam.sh`, `tools/seam-denylist.txt` | The seam guard and its denylist |
| `tools/lint-ecodes.sh` | The E-code guard: `Rue.Proto.Diagnostics` and the roadmap's table must agree |
| `tools/lint-rcodes.sh` | The R-code guard: every runtime code Appendix D documents is raised in the engine and asserted by a test |
| `tools/lint-goldens.sh` | Golden hygiene: no CR, no trailing whitespace, one trailing LF |
| `tools/lint-sdk-docs.sh` | The SDK docs guard: every SDK has user docs, and every example a page shows is the file its suite tests |
| `tools/lint-issues.sh` | The tracker guard: every issue is a file, and the index says what each issue says of itself |
| `tools/lint-cross-build.sh` | The cross-build guard: every member that compiles C is excluded from the cross-target builds, and every exclusion names a real member |
| `sdk/` | The embedding SDKs of 7.11 -- `rust/`, `python/`, `elixir/`, `java/`, `dotnet/` -- and `shim/`, the `rue-hook` shim; each has user docs in its own `docs/` |
| `tools/rediscovery/` | The rediscovery battery: a table of protections, a patch reverting each, `run.sh` to prove the suite catches every reversion, `check-patches.sh` in the gate so no patch rots |
| `Cargo.toml`, `core/` | The Rust workspace and `rue-core` (Phase 1): the model and its plan-IR shape, the checker, the verdict and its prose, `explain`, the state machine, the ledger; pure, no I/O |
| `cli/` | `rue`, the operator CLI: the whole verb list of section 6.8, over a `.rue` file for the offline verbs and over the control channel for the rest; exit codes per that section |
| `surface/` | `rue-surface` (Phase 2): the lexer, the lossless tree, the parser, `rue fmt`, and the resolver that turns a `.rue` text into the checker's input for one host |
| `render/` | `rue-render` (Phase 1): the target-side backstop artifact in POSIX `sh`, PowerShell and Python, every value baked in and quoted for its family |
| `engine/` | `rue-engine` (Phase 3): the clock, the store, the journal and its sinks, the executor seam, the lifecycle, footprints and drift, backstops and schedulers, gates and proofs, secrets, the control channel and the hook protocol |
| `bindings/` | `rue-bindings`: the generic built-ins and no more -- `local()`, `ssh()`, `cron()`, `task_scheduler()`, `launchd()`, `file()`/`stdout()` journals, `key()`, `always()`, `requester()`, `hold()`, `stdout()` notify |
| `daemon/` | `rued`: the daemon over a site block, its rc.d, systemd and `sc.exe` installation files |
| `tree-sitter-rue/` | The grammar for editors: `grammar.js`, the parser generated from it, highlight queries, and a guard that holds it to the front end over every `.rue` text here |
| `lsp/` | `rue-lsp`: the language server -- the checker's diagnostics as a text is edited, and hover on a step |
| `sim/` | `rue-sim` (tier 7): seeded event lists against a real engine, the twenty invariants of 10.3 after every event, and a shrinker over the events that broke one |
| `tenants/e2e/` | The tier 5 and 6 harness: provisioning for a disposable guest, and the scenarios of the roadmap's task 14 against real hosts |
| `tenants/t1/fixtures/` | T1's hooks as a daemon-spawned child: the authority, the escrow and the management-controller simulator, standard library Python |
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
