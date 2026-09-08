# Design

How the Phase 1 crates are put together: what each one is for, the shape of
the data that flows between them, the forms that are canonical, and the
contracts Phase 3's engine inherits. The plan of record is
[`ROADMAP.md`](ROADMAP.md); every rule is stated once there, and this document
points at the rule rather than restating it.

## Crates and the direction of dependency

```
core/      rue-core     the model, the checker, the verdict, the journal model,
                        the digests, the state machine; no I/O
render/    rue-render   the backstop artifact per artifact language; depends on
                        core only; no I/O
engine/    rue-engine   the runtime (ROADMAP 7): the clock in this unit; the
                        store, lifecycle, executors, arming, the control
                        channel and the hook protocol by Phase 3's units
cli/       rue          check | explain | artifact | states over a plan IR file
tenants/harness         rue-tenants: the tenants and negatives as terms, the case
                        table, the golden writer (rue-goldens) and the tier-2
                        and tier-3 suites
tenants/e2e             rue-e2e: the tier 5 and 6 harness, run on a disposable
                        reaper guest by tenants/e2e/run.sh and never by the
                        gate
proto/                  the Phase 0 Haskell prototype, kept as the record; its
                        tier-1 and tier-4 tests run in the gate
```

Dependency direction is downward only (ROADMAP 4.2). `rue-core` depends on
nothing in the workspace and on no I/O crate; `rue-render` on core;
`rue-engine` on core, render and surface; the CLI on all of them; the
harnesses on the public API and on nothing else. Per-OS knowledge
lives in `rue-render` (templates and quoting), `ci/build-target.sh`
(toolchains and linkers) and, from Phase 3, the generic executor and
scheduler bindings; the one exception is `core/src/artifact.rs`, the
vocabulary that says which shell family an `os` implies and which artifact
languages have a template, because the checker refuses a plan with no
template at check time (E0403) and core cannot depend on render.

## The front end

`rue-surface` (docs/LANGUAGE.md is the reader's guide): a logos lexer, a
hand-written recursive-descent parser with statement-level recovery
emitting rowan's lossless tree (`surface/src/parser.rs`), a formatter that
is the identity on the canonical layout (`fmt.rs`), the tree lowered to
plain data (`ast.rs`), and the resolver (`resolve/`): a file and its
imports to one `PlanIr` per host, the site derived from the block and the
inventory it names, clauses dispatched on the host's contract facts, every
step's op expanded at its call with the parameters bound and every
reference classified by origin, bodies lowered to primitives, footprints
to shapes by one rule, roles filling slots, protocols expanding to their
impls, `defprim` calls to classed templates. Its diagnostics are core's
`Diagnostic` type; the CLI renders them through miette.

## The model and the checker

`core/src/model.rs` is section 5 as Rust types with serde: facts and
footprints, ops with bodies, plans and items, gates, triggers and backstops,
the site. `core/src/body.rs` is section 6.4's primitives and references: a
body is a list of primitives, a value is a literal, a reference or a
template of both, and a reference names its origin (a fact on the target, a
plan parameter, a host field, an earlier step's output, a controller value,
a `secrets from:` binding). Secrecy and closure are read from that structure;
no flag says either.

`check(site, requester, plan)` in `core/src/check.rs` is pure and total: it
returns a `Verdict` (`core/src/verdict.rs`), never an error. The analyses it
composes are their own modules, each pure: `interference` (section 5.7, as
iterator joins shaped as the Datalog), `closure` (E0202), `secrets`
(E0206, E0209, E0210, E0211, E0606), `backstop` (coverage, the reach rule,
triggers by intent), `intent`, `gates` (satisfiability, minimum humans, the
requester's exclusion), `algebra` (numbering, leaves, the reversal laws).
`prose` renders the one-sentence verdict (Appendix A) and `explain` the
listing (Appendix B); both read only the verdict and the plan.

## The plan IR

The checker's input as data, `docs/TESTING.md` "The plan IR": one
canonical-JSON document holding the site, the requester and one host's
plan, `ir_version` 4 (3 plus the plan's probe declarations). It is `rue_core::model`'s serde form, spelled field by
field, with unknown fields refused. The tenants' terms are the IR's only
emitter until Phase 2's front end; `core/tests/ir.rs` holds a document that
exercises every primitive and reference and round-trips byte for byte.

## Canonical forms

Two encodings are canonical and stated in `docs/TESTING.md`: canonical JSON
(sorted keys, two-space indentation, integers only, one trailing LF) for
every golden and every IR document, so a golden's bytes are its meaning;
and the tagged canonical bytes of `core/src/canon.rs` for everything that is
hashed (one tag byte per value, so `None`, `""` and `[]` never collide;
records in declared field order; a message is its domain string then the
record). The journal (`core/src/journal.rs`, section 5.10) chains entries by
SHA-256 over the previous hash and the entry's canonical bytes; the request
digest and its plan, step and ack scopes (`core/src/request.rs`, section
5.11) hash the same way. Nonce and time are caller inputs; core has neither.

## The state machine and time

`core/src/states.rs` is section 5.9's five rules over an injected `now`
(`model::Instant`): expiry is a closed boundary, renewal is accepted only
within `renew_within` of the deadline and never for an expired plan; the
transition table is generated and is itself a golden
(`docs/state-transitions.tsv`). `core/src/ledger.rs` holds the cross-plan
reservations of section 5.12.

## The engine

`engine/` (rue-engine) is the runtime of ROADMAP section 7, arriving by
unit. What is in place:

- **The clock** (`engine/src/clock.rs`): a trait every module reads time
  through. `SystemClock` is the wall clock in whole seconds; `FakeClock` is
  set and advanced by tests and the simulation, in either direction.
- **The store** (`engine/src/store.rs`, 7.1 and 7.13): a directory with a
  `schema` file, a `lock` held exclusively for the daemon's life (`flock`;
  an exclusive open on Windows), `instances/<id>.json` in canonical JSON,
  `ledger.json` and the engine's own copy of the chain in `journal.ndjson`.
  Every write goes to a temporary name beside the file and is renamed after
  a sync. An unknown or missing schema is R0502; `rued migrate` is the only
  migration, dry-runnable, refused on a store another account owns, and
  recorded in `migrated.json` for the daemon's next start to journal.
- **The journal** (`engine/src/journal.rs`, 7.6): entries chained by core's
  `append`, optionally signed (SSHSIG, Ed25519, namespace `rue-journal`,
  `engine/src/sign.rs`), written to the store, then delivered to every sink
  synchronously; a sink that does not acknowledge is R0304, the refusal is
  chained after the entry and delivered to the sinks that still
  acknowledge, and the plan refuses to proceed. `rue journal verify`
  checks a file's chain and, with `--key`, every signature.
- **The executor seam** (`engine/src/executor.rs`, 7.2): one object-safe
  trait for everything done to a host. The engine hands it a resolved body
  (`engine/src/resolve.rs`: every reference already a string with its
  secrecy), so an executor never sees where a value came from. Empty output
  where an op promised one is `Silent`, a refusal. The fake records every
  call, runs a scripted outcome per body, and keeps file facts so a restore
  can be checked end to end.
- **The executors** (`bindings/src/local.rs`, `bindings/src/ssh.rs`, 7.2,
  7.4): `local()` runs on the controller with `env:` on the child process
  and `stdin:` piped, files in process, regions by `engine::region`'s rule;
  `ssh()` drives the system OpenSSH client through a transport seam (a fake
  in the tests), with `-F none`, only the declared identity and
  `known_hosts`, and every remote operation one `sh` reading its script
  from stdin: the stdin preamble is octal-escaped assignments decoded by
  `printf '%b'` inside that script, never `SendEnv`, never argv, and the
  script carries the artifact's own helpers (`rue_render::sh_helpers`). A
  probe's command answers a guard by its exit status (0 yes, 1 no, else
  unknown), its stdout the fact; a `run` binds a declared output with a
  stdout line `rue-output NAME=VALUE`. Stdout and stderr reported back are
  scrubbed of every secret the body carried. The host lock is `flock` on
  `<rue_root>/lock` locally and a long-lived `lockf` (FreeBSD) or `flock`
  (Linux) over ssh; a family with neither in base has none (macOS).
- **Footprints at runtime** (`engine/src/footprint.rs`, 4.3, 5.2, 7.7):
  before `do`, snapshots of `Modified` and `Region` files (to the record
  and to the instance directory); after `do`, the digest of every file
  fact of the plan on that host outside the step's own footprint is
  compared with its digest before, and a change is R0201 (`FootprintViolation`,
  the step undone, the plan reverted); the step's markers (`<kind> <path>
  <sha256>`) and the host's manifest of regions are written. At undo time
  each file fact is read against its marker and decided by the artifact's
  rule (`footprint::decide`): unchanged undoes; changed clobbers under
  `:clobber` (`DriftClobbered`) or holds under `:defer` (`DriftHeld`, the
  instance DriftHeld, `--force=drift` to proceed); a region with damaged
  markers is restored whole from its snapshot unless another active
  instance holds a region on the file (the ledger says), in which case it
  defers. The undo of a step with a region runs under the host lock, from
  the decision through the write and the marker's removal. An instance
  directory is created on a run-capable host before its first step, only
  when the host is bootstrapped (R0407); a `:target` undo on a host without
  a filesystem is refused before `do` (R0408); staged files are removed
  after their step and at boot for any instance not applying; the
  directory goes at close or commit. `rue bootstrap` prints the commands a
  target lacks, per family, and runs nothing; `rue doctor` reports every
  host's reach and bootstrap, the sinks, signing and settle.
- **The lifecycle** (`engine/src/lifecycle.rs`, 5.9, 7.1, 7.8): the driver
  over core's `states::transition`. Events come from verbs, from a step's
  outcome, or from the reap pass observing time. Progress is a set (the
  applied leaves with their repeat iteration, and the arm each `when`
  chose), not a cursor: the plan is walked from its start every time,
  skipping what is done, so a walk after a crash makes the same choices.
  The write-ahead `Applying{step, undo_line}` entry is acknowledged and the
  record persisted before a `do` runs. A failed step is undone at once; a
  refusal then holds (an earlier step with `refusal: :hold`) or reverts
  last-in-first-out, and a failing undo is `Stuck`, retried every pass.
  `:restore` undoes from the footprint: an owned file removed, a region
  stripped, a modified file written back from the snapshot taken before
  `do`. The reap pass observes the approval window, wane (before anything
  else), a wait's bound, an unknown guard, a handoff probe, and retries
  `Stuck`; boot demotes `Applying` to `Reverting`, reestablishes held
  resources (or suspends the instance), re-observes owned footprints, and
  only then leaves settle, during which no wane fires and no retry runs;
  the flag is persisted so a crash during settle stays settling. The
  request reserves every touched host's umbra in the ledger (R0101,
  R0203); a rehearsal journals every step, calls no executor and reserves
  nothing.

- **The control channel** (`engine/src/control.rs`, `engine/src/peer.rs`,
  docs/control-protocol.md): one Unix socket, newline-delimited JSON,
  identity from peer credentials mapped to the site's `operators` block
  (R0503; `:socket_owner`; `operator_for`; `admin`; `subscribe`), verbs
  scoped by plan (R0504) and admin (R0506), the version refused (R0501),
  hook registration on the same connection from declared registrars only
  (R0505), every connection and registration journaled. The handler is
  generic over the connection so the tests drive it over a socket pair.
- **The hook protocol** (`engine/src/hook.rs`, docs/hook-protocol.md): one
  link per connection or child, requests by id under a deadline (a miss
  is Silent, a missing field R0303), a registry by name, and adapters
  that present a hook as the engine's executor, journal sink or inventory,
  looking the link up at call time so an unregistered hook refuses
  honestly. Secrets have a place in exactly four messages.
- **rued** (`daemon/src/run.rs`): the site block to a daemon: sinks,
  signing key, hook executors, inventory, operators and registrars from
  `rue_surface::resolve::site_bindings`; the store created when empty;
  boot, then a reap thread and the accept loop; `--dry-run` for daemon
  dry-run mode; `--spawn NAME=COMMAND` for a hook child over stdio;
  rc.d and systemd files under `daemon/dist/`.

Positions the engine takes where section 7 is silent, for the owner: a
step gate, a knell's acknowledgement and an unknown guard all enter
`Waiting`, and a knell acknowledged up front (`--ack`) is journaled
`KnellAcknowledged` by the requester with no proof yet (the gates unit
brings the proofs); a `when` guard observed unknown takes the arm its
declared value names; a `repeat over:` list is read from a parameter, an
output, a controller variable or the owner host's probe, comma-separated;
an `observe` and a guard are answered by the owner host's executor; a
non-file fact under `:restore` with no snapshot is a failing undo, not a
guess; the `:controller` host is the machine the engine runs on, reached
by `local()`. The operators block's `user:` is required (E0602) since an
identity is a statement about an OS user; a `hello` without an identity is
admitted when the user maps to exactly one; a hook child spawned by the
daemon is the socket owner and needs a registrar declared so; `local()`
and `ssh()` executors arrive with the executors unit, so until then a
step on a host only they reach is deferred (a rehearsal is not).

## The backstop artifact

A `:target` backstop is a standalone script in the instance directory on
the target, registered with the host's scheduler, that undoes the covered
steps (those with `undo_locus: :target`) in reverse order when its trigger
is due (sections 5.6 and 7.7). `rue-render` produces it from the plan, the
host record, an `Instance` (id and `rue_root`) and the `Bindings` a request
supplies (parameters; host fields beyond name and os), in the host's
declared language: POSIX `sh` (FreeBSD, Linux, macOS), PowerShell
(Windows), or Python run by `uv run --offline --script` with PEP 723
metadata (any OS). Every value is baked in and quoted for the family that
reads it (`render/src/quote.rs`, section 6.4): interpolations in a `run`
string for the host's shell, then the whole text for the artifact's
language. What an artifact cannot carry is refused at render, not guessed:
a secret, a controller value, an earlier step's output, a fact read, a
controller primitive, `env:`/`stdin:` on a run, a held resource under
restore, a restore over a non-file fact. `rue artifact` prints it; the
tenants' artifacts are goldens beside their verdicts.

### The instance directory the artifact reads

This layout is the contract between `rue-render` and Phase 3's engine. The
engine writes everything but the last line; the artifact reads it and
writes the last line. Paths are relative to
`<rue_root>/instances/<instance-id>/`.

| Path | Written by | Content |
|---|---|---|
| `deadline` | engine, at arm and rearm | the epoch second the `after:` or `unless_confirmed:` trigger fires, as text |
| `heartbeat` | engine, every `interval` | the epoch second of the last heartbeat, as text; a "touch" is a rewrite |
| `markers/<n>` | engine, when step `n` completes | one line `<kind> <path> <sha256>` per file fact of the step, as `do` left it |
| `snapshots/<n>/<k>` | engine, before step `n` | the whole file for footprint entry `k` (`Modified` and `Region` entries) |
| `manifest` | engine | one line `region <path> <anchor>` per region this instance holds on the host |
| `artifact.sh` / `.ps1` / `.py` | engine, at install | the rendered artifact |
| `fired`, `drift`, `clobbered` | the artifact | `fired` once it has run; a step number per line as its policy decided |
| `<rue_root>/lock` | bootstrap | the host lock (7.7): `flock` for the engine, `lockf`/`flock` for a fired `sh` artifact, `fcntl.flock` for Python, an exclusive open for PowerShell; held for the whole of an artifact's run and across the engine's region undo |

Region markers in a file are the lines `# rue-region <anchor> begin` and
`# rue-region <anchor> end`; `region_set` writes them and the artifact
strips between them. A damaged pair is one line missing. Every file the
engine or the artifact writes is written to a temporary name beside it and
renamed (section 7.7), so a root scheduler job and a group-member engine can
each supersede what the other wrote. The heartbeat is compared by content,
not modification time: there is no portable `stat`, and the arm-time skew
check (section 5.6) already bounds the clocks.

Drift at undo (section 5.2) is decided by the artifact exactly as the
engine decides it: under `:defer` a file whose current digest differs from
its marker is left alone and the step is written to `drift`; under
`:clobber` it is undone and the step written to `clobbered`; a region
whose markers are damaged is restored whole from its snapshot unless a
sibling instance's manifest holds a region on the file, in which case it
defers. A non-file fact cannot be observed by a script and is undone as if
intact.

## The harness and the goldens

`tenants/harness` holds the case tables: `TENANT_CASES` and `NEGATIVES`
name each case's directory and the host, plan and requester its `.rue`
text is resolved for; `SURFACE_NEGATIVES` the texts the front end refuses.
`cases()` resolves every text through `rue-surface`; `artifacts()` is the
list of every expected file, computed from that; `rue-goldens` is the only
writer and refuses without `RUE_UPDATE_GOLDENS=1`. Every case yields
`plan.json`, `verdict.json` and `verdict.txt`; a tenant case also
`explain.txt`; a case with a `:target` backstop also its artifact; a
front-end negative `diagnostics.txt`. The Rust terms that carried Phase
0's record retired at Phase 2's exit, after the front end had been held
equal to each. `docs/TESTING.md` says what the suites may and may not do.

## Testing, in one paragraph

The gate (`tools/check.sh`) runs every guard and every suite and exits 0
only if every phase ran and passed. Tier 1 holds every rule to a raising
and a non-raising plan; tier 2 holds every golden byte for byte; tier 3
holds the guards to their self-tests; tier 4 holds the state machine, the
ledger and the seeded fuzz properties (`check`, `prose`, `explain`, the IR
and `render` never panic and their outputs are canonical). The rediscovery
table (`tools/rediscovery/table.tsv`) reverts each protection in a scratch
copy and requires the named test to fail. Mutation checks are run by hand
before a commit and are not automated.
