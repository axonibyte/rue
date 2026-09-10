# Testing

How rue is tested, what the tests are allowed to do, and what the suite
being green does and does not prove. This document is binding on the crates
and on the Phase 0 record the way reaper's `docs/testing-methodology.md` is binding on every
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
| `rcodes` | Every R-code of Appendix D is raised somewhere and asserted by a test, and no `"R0xxx"` literal exists outside the enumerations |
| `hook-ops` | The ops table of `docs/hook-protocol.md` and `OPS` in `rue-hook-proto` name the same 26 ops, in both directions |
| `golden-hygiene` | Every expected file has no CR, no trailing whitespace, exactly one trailing LF; JSON begins with `{` |
| `rediscovery-patches` | Every row of the rediscovery table names a patch that still applies to the tree, and every patch is listed |
| `darwin-deps` | No crate in the darwin dependency graph (`cargo tree --target *-apple-darwin`) is in `tools/darwin-denylist.txt`: the darwin binaries cross-link with zig and no macOS SDK, which a framework-linking crate would break |
| `tier3-selftests` | Each shell guard catches the fault it exists to catch, in a temporary tree |
| `cargo-fmt` | The workspace is rustfmt-clean |
| `cargo-clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` is clean |
| `cargo-build` | The workspace builds, every target, release, against the lockfile, under Rust 1.97 |
| `cargo-test` | The Rust suite passes, and changed nothing under `tenants/` or `docs/` |
| `cabal-build` | The prototype builds under GHC 9.10.3 with `-Wall -Werror` |
| `cabal-test` | The Haskell suite passes, and changed nothing under `tenants/` or `docs/` |

A phase whose tool is absent exits 77. That is a failure unless the caller
named the phase in `RUE_CHECK_SKIP_OK`. The FreeBSD reaper guest, which has
only the base system, declares the bash, shellcheck, cabal, cargo and
darwin-deps phases in `.reaper.toml`; the pipeline's GHC-image gate step
declares the cargo phases and darwin-deps, which its Rust-image step
performs; nowhere else is anything skipped. A Rust toolchain that is
present but not 1.97 is a failure, not a skip: the workspace's `rust-version`,
the CI image and the gate say one minor so fmt and clippy output is
comparable everywhere.

One set of goldens with one writer: the Rust terms under `tenants/harness`
produce every expected file and the Rust suite compares them. The Haskell
prototype's own tier-1 and tier-4 tests run in the same gate as the record
of Phase 0; it neither writes nor compares a golden.

## Tiers

The roadmap's section 10.1 names seven tiers. Phase 1 has the four that run
on a workstation: the Rust tiers under `cargo test --workspace`, the
prototype's under one tasty suite, and both inside the gate:

| Tier | Group | What |
|---|---|---|
| 1 | Rust `core/tests/{canonical,canon,diagnostics,ir,laws,interference,gates,intent_backstop,check,render,journal,request}.rs`, `render/tests/{quote,render,execute}.rs`, `surface/tests/tenants.rs`, `cli/tests/cli.rs`, `engine/tests/{clock,store,journal,lifecycle,control,drift}.rs`, `bindings/tests/{journal,local,ssh}.rs`, `daemon/tests/migrate.rs`, `cli/tests/daemon.rs`; Haskell `Test.Canonical`, `Test.Diagnostics`, `Test.Laws`, `Test.Check` | The canonical encoder's bytes and round trip; the hash encoding's bytes; the code enumeration; the IR spelling; the reversal laws as properties; the interference rules one by one; every emitted code raised by one plan and not by its sibling; the prose and explain clauses; the journal chain and the digests; per-family quoting round-tripped through real unquoters; the artifact's covered set, order, triggers, primitives and refusals in every language; the `sh` and Python artifacts executed against a temporary instance directory (below); every `.rue` text under `tenants/` parsing clean, `fmt` the identity on it and idempotent; the resolver's rules each on a small file (`surface/tests/resolve.rs`); the performance bound (`surface/tests/bench.rs`); the CLI's verbs, selectors and exit codes; the engine over fakes: the store's lock, atomicity and schema, migration; the journal's chain through every sink, a refusing sink as R0304, signatures with the key and with nothing else; the lifecycle's scenarios (below); the file sink and the key binding; `rued migrate` and `rue journal verify` end to end; the control channel over a socket pair (identity, version, scope, admin, registration, a hook serving execute and probe, a silent hook, subscriptions); rued and rue over a real socket (a plan through a registered hook, every verb's line and exit code, a spawned child hook over stdio, daemon dry-run mode, a missing group) |
| 2 | Rust `tenants/harness/tests/{goldens,tenants}.rs`, `surface/tests/corpus.rs` | Every artifact byte-identical to its expected file, no orphans and none missing; the terms and the case table 1:1; every tenant clean and every negative refused with exactly its code; the section 8 claims as verdict fields; every artifact golden exactly its covered steps in reverse; every parser corpus snippet's tree dump and diagnostics byte-identical to its goldens; every front-end negative's diagnostics byte-identical to its golden |
| 3 | Rust `tenants/harness/tests/schema.rs` (plus the shell guards in the gate) | Every verdict validates against `docs/verdict-schema.json`; every declared property path is produced by some verdict |
| 4 | Rust `core/tests/{states,ledger,fuzz}.rs`, `render/tests/fuzz.rs`, `engine/tests/table.rs`; Haskell `Test.States`, `Test.Ledger` | The five state-machine rules over the generated table; the cross-plan ledger's reservations; expiry and renewal against an injected now; the seeded fuzz properties (below) |
| 5 | Rust `tenants/e2e/tests/{smoke,firewall,recovery,breakglass}.rs` | rue against real hosts on a disposable guest: a plan applied and reverted over a real sshd, a real packet filter, a real cron and real hooks (below, "The scenarios") |
| 6 | Rust `tenants/e2e/tests/recovery.rs` | The same, with something killed: a daemon inside a step's `do`, a daemon before its backstop fires, an operator racing the target |
| 7 | Rust `sim/tests/sim.rs` | The shadow world: seeded event lists against a real engine, the twenty invariants of the roadmap's 10.3 after every event, and a shrinker over the events that broke one (below) |

`tenants/harness` (`rue-tenants`) holds the tenants and the negatives as
Rust terms, the case table as code (`TENANT_CASES`, `NEGATIVES`,
`EMITTED_CODES`), the golden plumbing, and the writer; it lives under
`tenants/` because it names tenants. The Haskell prototype is the Phase 0
record: its tier-1 and tier-4 tests still run in the gate, but it no longer
compares against or writes the goldens.

## The lifecycle over fakes

`engine/tests/lifecycle.rs` runs the engine over a fake executor per
transport, a memory sink and a fake clock, and reads back the journal the
sink received and the commands the fake ran: a temporary plan applies in
order and rests; the write-ahead entry is acknowledged before the step
runs (a sink that counts the fake's calls at delivery sees n-1 when entry
n arrives); a failing step undoes itself, then the prefix, and releases the
ledger; a failing undo is stuck, retried each pass, abandonable; an
executor that promises an output and says nothing is a refusal; wane
expires at the instant and reverts; renewal is within the window, never
after expiry, anchored at renewal; a permanent plan confirms and commits
and a temporary one refuses commit; a gate is pending and its window
lapses fail-closed; an unknown guard waits, a yes continues, a no refuses,
a lapsed bound reverts, a forced name passes; a refusal after a holding
step holds and resume retries; a step no transport reaches is deferred and
`handoff-done` (verb or probe) continues; a `:restore` undo removes,
strips and writes back exactly; a rehearsal calls no executor and reserves
nothing; a held exclusivity class is R0101 and an overlapping umbra R0203;
boot demotes an instance left applying; during settle no wane fires and
held resources are reestablished first, and the settle flag survives a
crash; a migrated store is journaled once.

`engine/tests/table.rs` is tier 4 through the driver: every applicable
row of the generated transition table (`docs/state-transitions.tsv`, 664
rows) is seeded as an instance in the store and its event fired through a
verb, `advance`, the reap pass or boot; the transition the engine records
must be the row's outcome, and a refusing row must come back as its
R-code. The events no code path of the current unit can produce are
named in the test with the unit that brings each, and the set is asserted
exactly, so a unit that makes one reachable must remove it there.

## The channel and the daemon

`engine/tests/control.rs` drives the connection handler over a Unix
socket pair in one process, the peer being the test's own uid, with
operators declared for that user, for another user, and for the socket
owner: identity by peer credentials (R0503 for a foreign identity, an
undeclared user, or an ambiguous unnamed hello), R0501 for another
protocol, a verb before hello refused, every verb within scope and R0504
outside it, R0506 for abandon by a non-admin, registration refused before
hello, outside `may_register` (R0505) and under another protocol,
accepted from the declared registrar and journaled, a hook serving execute
and probe on the same connection, a hook that goes silent refusing the
step and its departure journaled, a subscriber receiving its plan's
entries before the reply, daemon dry-run mode forcing rehearsals, and a
registered hook acting as an operator on its own connection.

`cli/tests/daemon.rs` starts `rued run` on a site file in a temporary
directory with the test's own group as the socket's, registers a hook
through the channel, and runs `rue apply`, `status`, `renew`, `recant`,
`apply --dry-run` and `abandon` against it, asserting each verb's exit
code and that its line is the last on stdout; a child hook over stdio
(`tests/fixtures/stub-hook.sh`, POSIX sh, no JSON library) registers as
the socket owner and serves a plan, and one registering outside
`may_register` stops the daemon with R0505; daemon dry-run mode starts on
a site with no operators block that refuses outside it (E0604) and makes
every apply a rehearsal; a group that does not exist refuses to start.

## Footprints and the executors

`engine/tests/drift.rs` runs plans over the fake executor's file facts:
the instance directory is created before the first step and removed at
close, with markers, snapshots and the manifest in the layout the artifact
reads; an unchanged fact undoes; a changed fact is clobbered under
`:clobber` and journaled; under `:defer` it holds the instance (DriftHeld,
exit 8, umbras kept, wane leaving it, a plain recant R0103, `--force=drift`
reverting); a region with damaged markers is restored whole unless a
sibling instance holds a region on the file; a `do` that touches another
op's fact is R0201 and reverts; an unbootstrapped host is R0407 and a
`:target` undo on a host without a filesystem R0408; a staged file is
removed after its step and at boot; the region undo holds the host lock
from decision to write (the fake logs every lock, run and write in order);
the manifest is never written in place; `bootstrap` reports a family's
commands and runs nothing. The tier-4 table now drives `DriftOnDefer` too.

`bindings/tests/local.rs` runs `local()` against real files in a temporary
root; `bindings/tests/ssh.rs` runs `ssh()` over a fake transport and reads
the scripts it would send (a secret never bare, the artifact's helpers
carried, the family's lock tool), and asks the real client with no key and
no known host, which must fail before any command runs.

## The simulation

`sim/` (`rue-sim`) is tier 7: a seed makes an event list, the events go to
a real engine over the fake executor, scheduler, approval and acceptor,
and after every one the twenty invariants of the roadmap's 10.3 are
checked against the instance records, the journal, the ledger and the
state of the host. An event is something an operator, a target or the
clock does: a request, a proof, a tick, a reap, a reboot, a recant with
and without `--force=drift`, a hand edit of a fact, an artifact firing,
a scheduler entry lost, a confirm, a commit, an abandon, an executor that
breaks. The world is two plans cut from the shapes T1 and T3 have: a
temporary one with a plan gate, a secret output, a region and a
`modified` fact under `:defer`, and a permanent one with a knell, a
region on the same file under another anchor, a confirm and a commit.

The suite runs a fixed sweep (seeds 1 to 39, 24 events each) so the gate
is deterministic; `RUE_SIM_SEED` and `RUE_SIM_STEPS` run one longer
scenario. A run that breaks an invariant is shrunk by delta debugging to
the shortest event list that still breaks *the same* invariant, and
reported with its seed, so a failure is a reproduction.

Two things the simulation does not do, said plainly. It applies the
artifact's rule to the shadow rather than executing the rendered script:
what an executed artifact does is proven where a real one runs, in
`render/tests/execute.rs` under `sh` and Python and in the end-to-end
harness where a real cron fires a real artifact. And four of the twenty
invariants this world cannot reach are named in the test that says so,
each with where it is proven instead: no wane during settle, no staged
file surviving, a proof scoped to one scope, and no act by an undeclared
identity.

Two exemptions are part of the contract rather than gaps. Between an
artifact firing and the engine's next contact, the target has undone
steps the engine still calls applied; that is R0402 read on the next
contact, and the exemption ends when the firing is journaled. And an
abandoned instance keeps its applied steps and its facts, because that is
what abandon means.

## Gates, proofs and secrets

`engine/tests/gates.rs` runs plans over a fake approval binding: a plan
gate holds an instance at Pending (exit 6) until proofs from two humans
arrive, and the journal names the authenticator and the submitter
separately; the plan, step and ack digests of one request all differ, and
another request's differ again, so no proof crosses a scope or a request;
a wait factor accrues and the reap pass opens the gate with no further
proof; a refused proof is `Denied` and accumulates nothing; a host
contract that changes between the request and the approval is R0301 and
takes every proof with it; a step gate waits for a proof of its own step
and is not opened by the plan's; a knell waits for an acknowledgement that
carries both a reason and a token the binding accepts, and a refused token
leaves the knell shut with nothing run; and a `:hold` under `mode: :auto`
is not refused but reverts at wane.

`engine/tests/secrets.rs` runs a plan whose op declares a secret output
over fake acceptors: the first acceptor that takes it ends the delivery
and the second is never offered it; the value reaches neither the journal
nor the record the store holds; a list every acceptor declines is exit 7
with `SecretUndelivered`; a held secret is revealed once and dropped when
the instance reverts; a hold drops at its bound on the reap pass and a
restart drops what is left, journaled `daemon_restart`; and a
`hold(until: :wane)` on a permanent plan with no `max_wait` is R0104.
`bindings/tests/secrets.rs` tests the real `requester()` and `hold()` on
their own. `engine/tests/control.rs` checks the R0305 guard at the hook
seam: `execute.run` and `secrets.deliver` keep a secret, every other
message loses the value and keeps its shape. `cli/tests/daemon.rs` starts
`rued` against a site declaring `approval via: always()` and finds it
refused, then admitted with `--dry-run`.

## Backstops, arming and reconciliation

`engine/tests/backstop.rs` runs plans with a `:target` backstop over the
fake scheduler and the fake executor: the artifact is installed into the
instance directory before the first covered step and armed before the step
`arm_before` names, which on the target is the `deadline` landing before
that step's `do`; a plan that arms late arms after its last covered step; a
host whose scheduler the site never bound is R0401 and the plan reverts; a
target clock ten minutes from the controller's is R0403 and nothing is
armed; an instance directory with the wrong modes is R0406; a renewal
rearms before the new expiry is the instance's, and a rearm the scheduler
refuses is R0404 with the old expiry standing; `confirm()` takes the
deadline away while a heartbeat trigger keeps the entry, and `commit()`
takes the entry before the directory goes; a temporary plan may carry a
heartbeat beside its `after:`, and the beat is written at arm and then at
its interval; a `fired` marker is read on the next reap and journaled
`BackstopFired` per step the target undid, which are no longer applied;
boot leaves an armed orphan where it is (`InstanceDirOrphaned`) and
reclaims a fired one; `rue reclaim` is refused while the artifact is armed
with its entry present (R0405), refused when forced without a reason, and
journaled `Reclaimed{forced: true}` when both are given; `abandon` names
what it could not disarm.

An artifact the abandon could not disarm is journaled
`BackstopFiredAfterAbandon` when it fires, once and not twice, and `rue
doctor` lists the armed orphans reconciliation left behind.

`bindings/tests/schedulers.rs` reads what each scheduler binding would run
on a target: `cron()`'s fenced crontab region, edited between a host lock
and its release, its arm and rearm writing nothing, its presence read from
a probe's exit status three-valued; `task_scheduler()`'s one task per
instance and its refusal of an argument it cannot quote; `launchd()`'s
property list beside the artifact. `cron()` is executed for real on the
guests by the e2e harness; the other two are executed nowhere.

## The hook protocol and its SDKs

`docs/hook-protocol.md` is the wire and `hook-proto/src/op.rs` is that wire
as data: 26 ops across 8 kinds, each with the fields its request carries,
the fields an `ok: true` reply must carry (R0303 otherwise), and whether it
is one of the four messages a secret may travel in. Everything that speaks
the protocol -- the engine's adapters, `rue-hook-sdk`, the shim, and the
conformance runner -- builds its frames from that one table, so freezing
the protocol at v1 is freezing one array.

`rue sdk-conform <command>` judges one hook against it. It spawns the
command with the same handshake `rued` performs for a `--spawn` child
(`rue_engine::hook::spawn_stdio_hook`, which both callers share), then
drives every op of every kind the hook registered for, against the fixed
world of `docs/sdk-conformance.md`. Exit 0 when every case passed, 1 when
any failed, 2 when the hook never registered -- a hook that ran and failed
has been judged, and one that never started has not.

A hook's command is run through the host's own shell (`sh -c` on unix,
`cmd /C` on Windows), because `--spawn NAME=COMMAND` is written in whatever
the operator's machine speaks and `rued` runs on all three families.

A hook is stopped by closing its stdin, never by signalling it. The process
spawned is the shell, which need not be the process that serves: signal that
one and the shell is reaped while the hook lives on as an orphan still
holding the stdout pipe, so whoever reads it waits for an end that never
comes. Every hook's serve loop ends at EOF on stdin, so
closing it stops whichever process is really serving; a kill is the fallback
for one that will not go.

Every reply is checked twice: against the case's own expected answer, and
against its op's row -- the id comes back, `ok` is a boolean, an `ok: true`
carries every required field, and it arrives inside the deadline. The
second check is most of what an SDK is for: a hook written on one cannot
answer `ok: true` without a required field, because the SDK builds the
reply from the row.

The suite also drives four **provocations**, which a conformance hook must
implement by deliberately violating the protocol: a refusal, an `ok: true`
missing a required field, a reply with no boolean `ok`, and no reply at
all. They prove the runner detects what the engine would (R0303, R0303 and
Silent), and they cannot be written through the SDK -- `Hooks::answer`
cannot express them -- which is why `sdk/rust/src/bin/conform_hook.rs`
drops to the wire for exactly those four and for nothing else.

Three enumerations are bound together so none can drift: `tools/lint-hook-ops.sh`
(the `hook-ops` phase) ties the document to `OPS`, and
`sdk/rust/tests/conform.rs` ties `OPS` to the cases the suite actually
drove. An op that gains a row fails the gate until it is documented and
fails the tests until it is driven.

## Goldens

The list of goldens is `rue_tenants::artifacts()`, computed from the case
tables and the `.rue` texts they name (each resolved by the front end for
its host, plan and requester), never from a directory listing. A
missing expected file fails; an expected file no artifact claims fails
("orphan"). The suite is read-only: the only writer is
`RUE_UPDATE_GOLDENS=1 cargo run -p rue-tenants --bin rue-goldens`, which
refuses without the variable (a test proves it refuses and touches nothing);
a case whose plan has a `:target` backstop also yields its artifact
(`artifact.sh`, `.ps1` or `.py`), rendered for instance `golden` with the
family's default root and the steps' arguments as parameters,
and the gate checksums `tenants/` and `docs/` before and after both test runs
and fails on any change. A mismatch prints the first differing line with
context and writes the actual bytes under `target/golden-actual/<path>` for
diffing.

Regenerating goldens is a decision, not a fix. Read the diff. If the change
is intended, the commit body says why the verdict changed.

## The parser corpus

`surface/tests/corpus/` holds one `.rue` snippet per construct of the
surface (the site block, every definition and body line, every item,
every expression form) and one per recovery (a missing `end`, a stray
token, an unterminated string, a tuple, several errors on separate lines,
the version marker missing, newer, malformed), each beside its tree dump
(`.tree`, every node and token with its range, whitespace elided) and its
diagnostics (`.diag`, one rendered line each). The test holds the tree's
text to the source (the tree is lossless), the goldens byte for byte,
`fmt` the identity and idempotent on every clean snippet, and `fmt`'s
refusal of every error snippet. The goldens are read-only in the suite;
`RUE_UPDATE_GOLDENS=1 cargo test -p rue-surface --test corpus` rewrites
them, the same variable as the tenants' writer, and the diff is read the
same way.

## Diagnostic goldens

Every code the front end raises has a negative directory under
`tenants/_negative/` holding the `plan.rue` that provokes it and, under
`expected/`, `diagnostics.txt`: the rendered diagnostics with paths
relative to the repository root. `rue_tenants::SURFACE_NEGATIVES` is the
table; `rue-goldens` writes the files by resolving the text; the tenant
suite holds each text to exactly its code, and holds the fifty-six codes
of section 6.7 to a partition into the checker's (`EMITTED_CODES`), the
front end's (`SURFACE_CODES`), the renderer's (`RENDER_CODES`) and the
unmodeled with a reason each (`UNMODELED_CODES`), so a code can be in no
list and in no two.

## The texts as the source

From Phase 2's exit the `.rue` texts are the golden source: `cases()`
resolves each through the front end for the host, plan and requester its
table row names, and every verdict, listing and artifact golden is what
core and render say of that. The Rust terms that carried Phase 0's record
were held structurally equal to the front end's plan IR for every case
before they retired (the equality test went with them; its proof is that
the goldens did not move when the writer switched sources). A text that
does not resolve fails every suite that reads it. `rue_tenants::text_of`
finds a case's text; the negatives are checked as the requester
`requester`, which the ones derived from a tenant need for E0508.

## The performance bound

`surface/tests/bench.rs` writes a 1,000-host inventory and a 200-step
plan to a temporary directory and holds parse, resolve and check to the
acceptance's bound (2 s; 5 s on FreeBSD) in release, which is how the
gate runs the tests; a debug `cargo test` (the workstation, the
rediscovery battery) asserts twice that and says so. The measured time is far under the bound (tens of milliseconds
in release); the rediscovery row `bench-over-budget` plants a stall.

## The artifacts run

`render/tests/execute.rs` executes the rendered `sh` and Python artifacts
the way a scheduler will (`sh artifact.sh`; `uv run --offline --script
artifact.py`) against a temporary `rue_root` and instance directory built
by the test: completion markers with real digests, snapshots, a deadline or
heartbeat file, a sibling manifest where the scenario needs one, and a
temporary directory of facts the plan's shapes name. Every scenario (not
due; due, in reverse order, fired once; an unmarked step; drift `:defer`
and `:clobber`; damaged region markers with and without a sibling; stale,
fresh and absent heartbeat; a hostile value through the quoting) runs in
both languages, so the two templates are held to one behavior. The tests
are `#[cfg(unix)]`: the artifacts they execute are the POSIX ones and the
windows-gnu suite under wine has neither `sh` nor `uv`. `sh` and `uv` (with
a cached interpreter: `uv python install 3.12`) are required on a gate
host, never optional; the pipeline and the reaper run install them. The
PowerShell artifact is a golden and a quoting unit test only: no gate host
runs PowerShell.

## Fuzz

`core/tests/common/gen.rs` is a seeded generator over whole sites and
plans, every item kind, op field, undo form, primitive, reference, gate
shape and trigger; `render/tests/fuzz.rs` includes it by path. The
properties (`core/tests/fuzz.rs`, `render/tests/fuzz.rs`): `check` never
panics and its verdict's JSON survives its canonical bytes; `prose` and
`explain` never panic; the plan IR round-trips; `render` never panics for
any host and never bakes a secret label, and at least one plan in twenty
renders so the property is not vacuous. `RUE_FUZZ_SEED` and
`RUE_FUZZ_STEPS` (default 500) override the defaults; a failing step is
reported with the rng state that replays it alone. The rediscovery rows
that plant a panic run at `RUE_FUZZ_STEPS=5000` through the table's env
column.

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
an integer, currently 3; a reader refuses any version it does not know. While
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
computed from the structure. A host record carries `stdin_preamble` and, from
version 3, `artifact` (`null` for the host's native shell, else `sh`,
`powershell` or `python`); the site carries `secrets_deliver_to`; from
version 4 the plan carries `probes`, each a declaration with its locus,
body, produced facts, `static` flag and equivalence, so the engine can run
what a guard names. `undo_idempotent` remains a stand-in until E0208's
analysis exists. `core/tests/ir.rs` holds a document exercising every
primitive and asserts it reads and writes back byte for byte.

`rue check <plan.json>` reads one.

## Negative cases

A negative case is a plan the checker must refuse with exactly one named
code. `tenants/harness/tests/tenants.rs` requires the set of codes across
the negative goldens to equal `rue_tenants::EMITTED_CODES`, in both
directions: a new check without a negative golden fails, and a negative
golden for a code the checker cannot raise fails. The twelve of the
roadmap's Phase 0 task 8 derive from T1 and T3 by one change each; the rest
are minimal plans on the lab site described in
`tenants/harness/src/tenants/negative.rs`. Every negative has a `plan.rue`
beside its goldens, the text Phase 2 must refuse the same way.

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

`.reaper.toml` declares two guests. `reaper up && reaper test`; the manifest
is validated by `reaper doctor`.

- **ubuntu-26.04.** Its `build` runs in the digest-pinned Rust 1.97 image the
  pipeline uses (Debian trixie) with GHC 9.10.3 and cabal installed by ghcup
  into the guest's caches on first use: the cabal and cargo builds, then the
  whole gate, then the Windows suite under wine. Its `run` executes on the
  guest itself (`exec = "host"`), because the tier 5 and 6 harness needs a
  real sshd, nftables and cron and the container has none of them and no
  capability to add them: it installs a pinned 1.97.1 toolchain from rustup
  into a cache of its own and runs `tenants/e2e/run.sh`.
- **freebsd-15.1.** Host execution throughout. Its `build` installs `uv` and
  a system Python from pkg and a pinned 1.97.1 toolchain from rustup (the
  port's rust is 1.96 and the workspace's `rust-version` says 1.97), then
  builds the workspace; its `run` is the gate with bash, shellcheck and cabal
  declared skipped, then `tenants/e2e/run.sh` against pf, sshd and cron.

rustup-init and ghcup are fetched to files and executed, never piped into a
shell. Every skip is declared in the manifest's `RUE_CHECK_SKIP_OK` and
nowhere else.

### The scenarios (tier 5 and 6)

`tenants/e2e/tests/` holds them, one file per theme, each starting a real
`rued` over a site of its own beside the harness's key material and
driving it with the real `rue`:

- **The firewall** (`firewall.rs`): a plan opens a port by a fenced region
  in the host's packet-filter file and reloads it, then commits when
  confirmed, leaving the region and the rest of the file untouched; a
  temporary plan of the same shape is recanted and the region goes; a
  `modified` fact edited on the target behind the engine's back holds the
  instance at exit 8 with R0202 and leaves the stranger's edit alone until
  `--force=drift` restores what the step found; a `do` that writes outside
  its step's footprint is R0201 and the plan reverts; and a journal one
  entry has been deleted from fails to verify and says where.
- **Break-glass** (`breakglass.rs`): T1's shape whole. A plan gate two
  humans must open through an approval hook, a management-controller
  account enabled through a second hook on a host with no filesystem, the
  secret it produces escrowed through a third, and a fenced block in a
  real authorized-keys file over a real sshd. The hooks are
  `tenants/t1/fixtures/hooks.py`, spawned by the daemon as stdio
  children. One proof leaves the instance pending; the second opens it;
  the escrow has the label and no journal has the value; a recant puts
  both hosts back.
- **Drift by either hand** (`firewall.rs`): the same step, the same edit
  behind the engine's back, the same `:clobber` policy, undone once by
  the engine on a recant and once by the artifact firing on its own with
  no engine anywhere. The file ends the same either way, which is the
  claim behind rendering the artifact from the footprint the engine
  reverts from.
- **Recovery** (`recovery.rs`): a daemon killed with SIGKILL inside a
  step's `do` comes back, demotes what it was applying, and undoes both
  the steps it had marked applied and the one it was in the middle of; a
  backstop armed by a daemon that then dies still fires from the target's
  own cron, with no engine anywhere, and undoes the step; a recant that
  races a fired artifact leaves the fact restored once rather than twice,
  because both take the same host lock and restore the same snapshot;
  `rue doctor` names the host, its transport and its scheduler; and `rue
  doctor --canary` installs a throwaway artifact of the engine's own on
  every host with a scheduler, arms it with a deadline already past, waits
  for the marker it leaves, and removes it whatever happened. That last is
  the one proof no unit test can give: that this host's cron runs what rue
  installs.

### Tier 5 and 6: the harness on a disposable guest

`tenants/e2e` (crate `rue-e2e`) holds the tests that run rue against real
hosts. They are never a gate phase: `tools/check.sh`, the pipeline and
`ci/test-windows.sh` all run `cargo test --workspace --exclude rue-e2e`,
with that reason beside the exclusion, and the harness's tests refuse
(panic) unless `RUE_E2E=1`, which only `tenants/e2e/run.sh` sets. A test
that can only pass by touching nothing is not a test, so a workstation run
of the crate fails loudly rather than reporting green.

`run.sh` refuses on any machine that is not a reaper guest (`REAPER_WORK`
unset) unless `RUE_E2E_DISPOSABLE=1` says it may be rewritten, provisions the
guest with `tenants/e2e/provision.sh apply`, asserts the provisioning with
`provision.sh check`, and runs the crate's tests one at a time. Provisioning
means: the harness's Ed25519 key and its own `known_hosts` under `rue-e2e`
beside the working tree (never inside it; never `~/.ssh`, which rue and
its tests read and write nowhere); an sshd drop-in adding that file as a
second `AuthorizedKeysFile`; the loopback alias `127.0.0.2` every e2e plan
addresses its target by, so a plan that severs ssh severs only itself and
never reaper's transport; a firewall baseline that skips the management
interface (pf `set skip`; an nftables table of rue's own whose input chain
accepts); a scheduler baseline that removes every `# rue-region` block an
earlier run left in the crontab, because reaper's reset rolls back the state
dataset and not `/var/cron`, so an entry outlives the instance directory it
names and would answer a later run's question about whether a backstop is
present (`apply` asserts its own strip, and `check` does not: `check` runs a
second time as a test of its own, once this run's backstops are armed and an
empty crontab would be the bug); the `rue` group; and `rue_root` under
`$REAPER_STATE/rue`, the dataset reaper's reset rolls back. Every ssh call the harness makes is
`ssh -F none -o IdentitiesOnly=yes -i <its key> -o UserKnownHostsFile=<its
file> -o GlobalKnownHostsFile=/dev/null -o StrictHostKeyChecking=yes`.

This unit's tier 5 is the smoke: the provisioning self-check passes and the
target answers over rue's own key through the alias (`SSH_CONNECTION`
names `127.0.0.2:22` on the server side). Every later case stands on it.

Windows is tested under wine: `ci/test-windows.sh` builds the whole suite for
`x86_64-pc-windows-gnu`, statically linked against the C runtime
(`.cargo/config.toml`) so the binaries carry no mingw DLL dependency, and
runs it with wine as cargo's runner, on the Ubuntu reaper guest and in the
pipeline's `doTestWindows` step. Rust's standard library needs
`bcryptprimitives.dll`, which wine has had since 8.13; Debian trixie's wine 10
qualifies and bookworm's 8.0 does not, which is why both hosts are trixie. That
proves the crates' logic and the CLI's bytes on the Windows target. What wine
cannot exercise -- services, named pipes, the Task Scheduler, ACLs -- is
proven on no real machine in Phase 3, by the owner's decision of 2026-09-07,
and the roadmap's not-proven table says so.

## What green does not prove

- Nothing about a construct no tenant or negative case uses.
- Nothing about the `.rue` text: it is unparsed until Phase 2, and only its
  existence per case is asserted; the terms are transcribed from it by hand.
- Nothing about hosts: no executor, no engine exists. The backstop artifact
  is rendered and, for `sh` and Python, executed against a temporary
  instance directory the tests build; that the engine writes that directory
  as `docs/DESIGN.md` states, and that a real scheduler runs the artifact,
  are Phase 3's. Tiers 5 to 7 begin there.
- The PowerShell artifact: rendered, golden-tested, quoting unit-tested,
  executed nowhere. `uv` on a target (present, interpreter cached, offline
  at fire time) is an arm-time precondition Phase 3 checks.
- On Windows, what wine can show, which turned out to be more than was
  assumed: the whole suite on the windows-gnu target, the control protocol
  over anonymous pipes, the service dispatcher's argument parsing and its
  stop handler, the host lock as a locked file, and a named pipe carried
  end to end -- created with its access-control list, connected, and the
  client named from its own SID through an impersonation, which is the
  identity model working on Windows. What no wine run can show, and Phase
  3W will: a real service-control manager starting `rued`, the kernel
  enforcing the list against a client that should be refused, the Task
  Scheduler arming and firing a backstop, and PowerShell as `local()`'s
  shell.
- On macOS, only an artifact golden: the darwin binaries are cross-built,
  clippy-clean and packaged, executed and signed nowhere until a Mac exists.
  `launchd()` is written and unit-tested against the fake transport and has
  never installed a job.
- `task_scheduler()` likewise: the commands are checked, no Windows machine
  has run one this phase.
- The heartbeat under a real network partition: the beat is written and
  read on one machine's clocks, never across a severed link (a vnet stage
  is Phase 5's).
