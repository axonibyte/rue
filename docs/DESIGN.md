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
plan, `ir_version` 5 (3 plus the plan's probe declarations, 4 plus the
fact shape a probe `reads`). It is `rue_core::model`'s serde form, spelled field by
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
  recorded in `migrated.json` for the daemon's next start to journal. An
  older schema is R0502 too, and says to migrate. Schema 2 (v0.2.0) is the
  first change to the record since v0.1.0: an applied step carries the
  repeat variables it ran with. A schema 1 record is a valid schema 2
  record, so 1 -> 2 rewrites nothing; what it cannot do is recover the
  variables of steps applied inside a repeat before it, which undo as
  schema 1 undid them, without. `engine/tests/fixtures/store-v0.1.0` is
  the store v0.1.0 itself wrote, the first upgrade vector of 7.13.
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
  before `do`, snapshots of `Modified` and `Region` facts (to the record
  and to the instance directory); after `do`, the digest of every
  observable fact of the plan on that host outside the step's own
  footprint is
  compared with its digest before, and a change is R0201 (`FootprintViolation`,
  the step undone, the plan reverted); the step's markers (`<kind> <path>
  <sha256>`) and the host's manifest of regions are written. At undo time
  each observable fact is read against its marker and decided by the
  artifact's rule (`footprint::decide`): unchanged undoes; changed clobbers under
  `:clobber` (`DriftClobbered`) or holds under `:defer` (`DriftHeld`, the
  instance DriftHeld, `--force=drift` to proceed); a region with damaged
  markers is restored whole from its snapshot unless another active
  instance holds a region on the file (the ledger says), in which case it
  defers. **An observable fact is one the engine can read back through the
  executor**, which is any `Owned` or `Modified` fact and those `Region`
  facts that live in a file: a region is the text between two markers
  inside a file by construction, and a region on anything else is not a
  fact to compare. So an appliance's reported state, reached through a
  hook, drifts and is watched exactly as a file does, and over `local()`
  or `ssh()`, which read files alone, a fact that is no file is read by
  the probe whose `reads` names its shape, run with the shape's names
  bound (`Engine::fact_bytes`, `footprint::bind_shape`). **A read that
  fails is R0205, never the fact's absence**: before `do` it refuses the
  step, after `do` it fails the step, at undo it fails the undo. Before
  `do` every observable fact's digest is kept with the snapshot, and a
  failed step whose facts all read as they did then is not undone
  (`UndoSkipped`): its `do` never took. `markers/<n>` on the
  host carries the file facts alone, because its other reader is a
  rendered artifact with no executor (below); the engine's own record
  carries every one, and the two therefore agree wherever the artifact can
  see and the engine sees further. The undo of a step with a region runs under the host lock, from
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
  applied leaves with their repeat iteration and variables, and the arm
  each `when` chose), not a cursor: the plan is walked from its start every
  time, skipping what is done, so a walk after a crash makes the same
  choices. An application of a step is its step, iteration and variables:
  the variables are what tell the passes of nested repeats apart, what its
  undo resolves against, and what its markers and snapshots are kept
  under (`step_key`). A runtime value in a fact's shape (`{g}`) is the
  value that application holds, in the footprint and in the body alike
  (`resolve::instantiate`), so an iteration touches, snapshots and
  restores its own fact.
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
  nothing. **A rehearsal's instance id is its own** (`<plan>.<host>.<hash>`
  with `.rehearsal` after it), because reserving nothing is only half of
  "a rehearsal never blocks a real plan" (7.9, D-085). The other half is
  the instance store: an id held by a non-terminal record is R0101, and a
  rehearsal ends `Applied`, which is not terminal -- so while the two
  shared an id, rehearsing a plan once refused the real one for good
  through a path no ledger rule touches, and rehearsing a running plan
  would have overwritten its live record with one that holds nothing.
  Separate ids also keep the journal readable: a rehearsal's only trace is
  its entries, and while they were filed under the real plan's id nothing
  told them apart from an apply that happened. The duplicate-instance
  guard does not apply to a rehearsal at all, for the same reason.

- **The control channel** (`engine/src/control.rs`, `engine/src/peer.rs`,
  `engine/src/pipe.rs`, docs/control-protocol.md): one channel,
  newline-delimited JSON,
  identity from peer credentials mapped to the site's `operators` block
  (R0503; `:socket_owner`; `operator_for`; `admin`; `subscribe`), verbs
  scoped by plan (R0504) and admin (R0506), the version refused (R0501),
  hook registration on the same connection from declared registrars only
  (R0505), every connection and registration journaled. The handler is
  generic over the connection so the tests drive it over a socket pair.
  The transport is the platform's: a Unix socket with mode 0660 and a
  group, or a Windows named pipe whose discretionary access-control list
  names the same group beside SYSTEM and the administrators, with the
  peer's account read from the client's SID rather than a uid. A group the
  system does not know refuses the daemon rather than widening the pipe.
- **Windows** (`engine/src/pipe.rs`, `daemon/src/service.rs`,
  `bindings/src/local.rs`, 7.9 and 12): `rued` registers with the
  service-control manager, answering `Interrogate` and stopping on `Stop`
  and `Shutdown` through the same flag `rue run` never sets; `local()`
  runs PowerShell with `-NoProfile -Command` and keeps its root under
  `%ProgramData%\rue`; the host lock is a locked file taken with
  `LockFileEx`, the same file the PowerShell artifact opens exclusively.
  All of it is built for `x86_64-pc-windows-gnu` and tested under wine,
  which carries the pipe end to end and names its client from that
  client's own SID; what only a real machine can show is named in
  docs/TESTING.md and in the roadmap's Phase 3W.
- **The hook protocol** (`engine/src/hook.rs`, docs/hook-protocol.md): one
  link per connection or child, requests by id under a deadline (a miss
  is Silent, a missing field R0303), a registry by name, and adapters
  that present a hook as the engine's executor, journal sink or inventory,
  looking the link up at call time so an unregistered hook refuses
  honestly. Secrets have a place in exactly four messages.
- **Embedding** (docs/control-protocol.md, docs/hook-protocol.md, `sdk/`):
  a host process embeds rue by holding a connection to `rued`, never by
  linking it (NIF embedding is deliberately not built). On one connection
  it is a declared operator issuing verbs within `operator_for`, a declared
  registrar whose hooks the engine calls back into within `may_register`,
  and a subscriber to its plans; `Client::read` queues an event that
  arrives while a verb is in flight, for `next_event` to drain, so a host
  alternates verbs and events on the one connection rather than splitting
  it. The wire is one table, `rue-hook-proto`'s `OPS`, frozen at v1 by
  `docs/hook-protocol-v1.json`; each SDK of 7.11 carries a transcription
  of it that `tools/lint-hook-ops.sh` binds to `OPS`, and `rue sdk-conform`
  judges any of them against the scripted world of docs/sdk-conformance.md.
  Every SDK hands an `execute.run` handler its resolved values as a type
  that formats and serializes a secret as `<secret>` and gives its text
  only through `expose` (7.11's "without ever placing them on a command
  line"), and bounds a handler by an optional budget, answering `ok: false`
  with the overrun when it is spent, because the engine's deadline is not
  on the wire for an SDK to see. `rue check --ir` gives an embedder the IR a terminal check produces, and
  the verdict the daemon reaches for an embedded apply is the one
  `rue check` gives on the same text. A hook may stand in for the
  controller: `hook(:x, transport: :controller)` puts the controller's
  actions in a host process, and E0608 refuses at check a plan with an
  action no executor it reaches can perform.
- **Backstops at runtime** (`engine/src/backstop.rs`,
  `engine/src/scheduler.rs`, 5.6, 7.7): the artifact rendered into the
  instance directory before the first covered step and registered with the
  host's scheduler; armed before the step `arm_before` names, which on the
  target is the `deadline` landing before that step's `do`; rearmed before
  a renewal's new expiry becomes the instance's (a rearm that fails
  refuses the renewal, R0404); the `unless_confirmed` deadline taken away
  by `confirm()` and the entry itself by `commit()`, `abandon` and a clean
  revert, always before the directory it reads is removed; the heartbeat
  written at arm and at its interval by a daemon thread; the `fired`
  marker read on the next contact and journaled `BackstopFired` per step
  the target undid (R0402), those steps then no longer applied. Arming
  refuses on a clock beyond `skew_tolerance` (R0403), on an instance
  directory whose modes are wrong (R0406), on a host whose scheduler the
  site never bound (R0401) and on a target not bootstrapped (R0407).
- **The scheduler bindings** (`bindings/src/cron.rs`,
  `task_scheduler.rs`, `launchd.rs`, 7.3): `cron()` edits one fenced
  region of the crontab, anchored by the instance id, under the host lock;
  `task_scheduler()` creates one task per instance; `launchd()` writes a
  property list beside the artifact. `hook(:name)` carries the five ops of
  the protocol's `scheduler` kind.
- **Gates and proofs** (`engine/src/gates.rs`, 5.11): the request digest
  over a 32-byte nonce from the platform's CSPRNG, the plan's content, its
  parameters, its wane, the instant of the request and the host contract;
  the scoped digests a proof binds to, so a proof for one step verifies for
  no other and none for the plan. The approval binding publishes its
  authenticators, renders a challenge over a digest and returns a verdict
  on a proof; a proof accepted is journaled `ProofAccepted{scope,
  authenticator, submitter}` and one refused is `Denied`. A gate is
  satisfied when some satisfying path has a proof from every authenticator
  on it and its wait has elapsed, which makes a wait factor weight that
  accrues: the reap pass opens a plan gate whose hour has passed with no
  further proof. The host contract is the `HostRecord` fields of every host
  the plan touches and every `static: true` probe on them, frozen at the
  request and re-derived at approval and at apply; a change is R0301 and
  every proof falls with it.
- **Secrets** (`engine/src/secrets.rs`, 5.13): a secret output is
  delivered when the producing step's completion is journaled, to the first
  acceptor of `secrets deliver_to:` that takes it, and the engine then
  holds nothing. `requester()` takes it only while a client is attached and
  hands it to that client's reply; `hold()` keeps it in memory, gives it up
  once to `rue reveal`, and drops it at its bound, when the instance ends,
  and at a restart. A list every acceptor declines is `applied; secret
  undelivered` and exit 7. Nothing about a secret reaches the store or a
  journal entry but its label, and a secret in a hook message other than
  the two that may carry one is dropped at the seam (R0305).
- **Reconciliation and reclaim** (`engine/src/backstop.rs`, 7.7): at boot
  every instance directory on every reachable host is compared with the
  store; one the store does not know that holds an armed, unfired artifact
  is left where it is and journaled `InstanceDirOrphaned{armed: true}`;
  one with no artifact or a `fired` marker is removed and journaled
  `Reclaimed`. `rue doctor` lists what was left in place, and `rue doctor
  --canary` proves a real backstop fires: a throwaway artifact of the
  engine's own (no plan, no undo, nothing outside its own instance
  directory), armed with a deadline already past and removed whatever
  happened. `rue reclaim`
  refuses while the artifact is armed and its entry present (R0405) until
  `--force --reason`. An artifact `abandon` could not disarm and that
  later fires is read on the next reap and journaled
  `BackstopFiredAfterAbandon`: accepted and visible, never a surprise.
- **rued** (`daemon/src/run.rs`): the site block to a daemon: sinks,
  signing key, hook executors, schedulers, inventory, operators and
  registrars from `rue_surface::resolve::site_bindings`; the store created
  when empty; boot with its reconciliation, then a reap thread, a
  heartbeat thread and the accept loop; `--dry-run` for daemon dry-run
  mode; `--spawn NAME=COMMAND` for a hook child over stdio; rc.d and
  systemd files under `daemon/dist/`.

**There is no separate per-instance lock**, and 7.7's "host and instance
locks" is met by the host lock alone. What a second lock would guard is
covered three ways: one daemon per store, because the store's own lock is
held exclusively for the daemon's life; an artifact and the engine on one
host, because the artifact takes the host lock for its whole run and the
engine takes it across any region undo, which is the only case where two
writers can corrupt one file; and two controllers acting on one host,
which the roadmap defers to Phase 5. The engine does not take the host
lock for *every* undo because `host_lock` opens a file bootstrap creates,
and a `:controller` step on a machine with no `rue_root` would then fail
to revert. A lock nothing takes would be worse than none.

The end-to-end work settled two things. **A step whose `do` the engine
died inside is undone on the way back.** The write-ahead entry exists so
the engine says what it is about to do, and how it would undo it, before
it does it; a death between those two leaves a step that may have half
happened and was never marked applied. The record now names the step in
flight (`attempting`), written with that entry and cleared when the step
ends, and boot recovery undoes it with the rest. Undoing a step that never
took is harmless, which is what makes an undo an undo; leaving one that
did is not.

**A closed instance is exit 1
only when something refused it.** Both a plan that reverted after a
refusal and a plan an operator recanted end `Closed`, and section 6.8
gives one code to "refused" and another to "ok"; the reason the record
carries is what tells them apart. So `rue recant` on a healthy instance,
`rue cancel` on a pending one, and a temporary plan that waned and
reverted cleanly are all exit 0, while a step that failed, an executor
that went silent, and an abandon of an instance that got there by
refusing are exit 1.

Positions the gates and secrets unit takes where the roadmap is silent,
for the owner. **The operator's own identity is the authenticator** a proof
is verified against unless `rue approve --authenticator` names another; the
submitter is always the channel's peer identity, and the journal keeps both.
**`rue approve` with nothing on stdin prints the challenge** rather than
submitting an empty proof, so the token a binding wants can be fetched with
the same verb that spends it. **The host contract is the host records plus
the static probes**, hashed as canonical JSON: the roadmap names its
contents (5.1) and leaves the shape to the engine. **An approval binding
that opens every gate is a property of the binding**, not a special case in
the gate evaluator: `always()` answers `approves_everything`, and `rued`
refuses to build it outside `--dry-run`. **A binding that fails while being
offered a secret has not accepted it**: the failure is journaled and the
next acceptor is offered the value, because a refusal to answer is not a
refusal to take. **A reveal is journaled** as well as the delivery, since
that is the moment the value reaches a person.

Positions the backstop unit takes where the roadmap is silent, for the
owner. **One artifact undoes one host's steps**, so a backstop whose
covered steps span two hosts is refused at apply and named; the roadmap's
rendering call takes a single host and 7.7's instance directory is that
host's. **The engine owns the instance directory and the binding owns the
entry**: the engine writes the artifact, the `deadline` and the
`heartbeat` through the executor and makes the skew probe, while
`install`, `disarm` and `present` are the binding's; `arm` and `rearm`
exist for a scheduler that holds the time itself, and for a periodic one
(`cron()`, `task_scheduler()`, `launchd()`, whose entries run the artifact
every minute while the artifact compares its own deadline) they do
nothing, which is what "self-enforced on `<host>`" in the verdict already
says. **Armed is the artifact's presence**, not the deadline's: a backstop
with only `unless_heartbeat:` writes no deadline file, and reconciliation
must not read that as reclaimable. **A transport that cannot ask a host
its time reports no skew rather than zero** (`Executor::clock_now`
answering `None`), so R0403 is enforced where it can be and its absence is
visible in `rue doctor` instead of assumed away; a hook that does not
serve `execute.clock` refuses it and is read the same way. **The
instance-directory modes come back with the listing** (`modes_ok`), which
is where the engine reads them for R0406. **A scheduler that cannot say
whether its entry is there is never read as absence**, so `rue reclaim`
refuses on `unknown` exactly as it does on `present`.

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
| `deadline` | engine, at arm and rearm; removed by `confirm()` | the epoch second the `after:` or `unless_confirmed:` trigger fires, as text |
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
intact -- which is the artifact's limit and not the engine's, and the one
place the two decide differently. §5.2 states the rule about facts; a
shell script can only look at files, so the artifact keeps the narrower
half of the rule and the engine keeps all of it.

## The simulation

`sim/` (rue-sim) is the shadow world of section 10: a seeded event list
driven against a real engine over fakes, with the twenty invariants of
10.3 checked after every event and a delta-debugging shrinker over the
events that broke one. It depends on core, render and the engine, and on
no binding: what it exercises is the engine's orderings, not a
transport's.

The positions it takes: an artifact that fires is the artifact's *rule*
applied to the shadow, not the rendered script executed, because what an
executed artifact does is proven where a real one runs; the invariants
this small world cannot reach are named in a test rather than omitted;
and two windows are exemptions rather than violations -- between a firing
and the engine's next contact the target has undone steps the engine
still calls applied, and an abandoned instance keeps its applied steps
and its facts, which is what abandon means.

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
