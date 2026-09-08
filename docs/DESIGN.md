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
plan, `ir_version` 3. It is `rue_core::model`'s serde form, spelled field by
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
