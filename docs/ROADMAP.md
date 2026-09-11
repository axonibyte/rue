# rue — roadmap and plan of record

**A language for provably reversible operations.**

> Undo as a compiler verdict. A rue plan is a sequence of operations whose
> reversibility — how far it can be undone, from where, by whom, and past
> which point it cannot — is decided by `rue check` before anything runs,
> and stated in one sentence.

This document is the plan of record, written for an implementer working
without the conversation that produced it. Every rule is stated once, in
the section that governs it. Where it says MUST, a test enforces it. §3
is an index of decisions; the rule behind each decision lives in the
section the index names, and that section is authoritative.

Status: **pre-Phase 0.** Nothing is built. This document is the first artifact.

---

## 0. How to read this, and the rules that bind every phase

### 0.1 Reading order

1. §0.4 and §0.5 — the tenant test and the authority rules. They decide what an implementer may and may not change.
2. §1 — the claim and its prior art. Know what rue is *not* before writing it.
3. §2 — vocabulary. Every term is used exactly as defined there.
4. §4–§7 — the model, the surface, the engine. The spec.
5. §8 — the four acceptance tenants: the *floor* of capability, not its ceiling.
6. §9 — the phases. What to build, in what order, with exit criteria.
7. §10 — the testing portfolio. How anything gets to call itself done.
8. §3 — the decisions index, for finding where a rule lives and why.

### 0.2 Engineering discipline (binding on all rue code)

- **Evidence before assertion.** A verdict states only what a check proved. "Cannot prove" and "cannot rule out" are distinct outputs, never collapsed.
- **Empty output with exit 0 is never evidence.** A probe, hook or command that promises output and produces none has violated its contract; every caller treats that as a refusal.
- **Fail closed.** Anything unresolved, unknown, unregistered, unreachable or unproven refuses. A missing declaration is a refusal to run, not a default.
- **Pure core, injected time.** `rue-core` performs no I/O and reads no clock. Every observation takes `now`. Expiry is a property of observation, not of a background thread.
- **Write-ahead everything.** Intent is journaled and acknowledged before a mutation begins. A crash between any two steps restarts into a state that either retries the undo or is demoted to needing one.
- **Every mutating step prints its undo before it runs** — in `explain`, in `apply` output, and in the journal.
- **Refusals are printed in the engine's words, verbatim, and exit nonzero.**
- **Diagnostics name what was expected and found**, with a nearest-name suggestion (edit distance ≤ 2) where one exists.
- **No tenant vocabulary in the framework.** A lint guard enforces it (§4.4).
- **Every phase ships a "what is NOT proven" table.** A claim without a living test is a claim this document does not make.
- `sh -n`, shellcheck, `cargo clippy -D warnings`, `cargo fmt --check` on every commit. `tools/check.sh` runs every check and reports every failure rather than stopping at the first.

### 0.3 What rue is not (scope fence)

- **Not a configuration-management or convergence tool.** Terraform, Ansible and Kubernetes converge toward declared state. Rue sequences operations and proves their reversal. Where convergence is the right model, rue is the wrong tool, and the README says so.
- **Not a scheduler or workflow engine.** No queueing, no placement, no durable execution beyond what write-ahead journaling needs.
- **Not a secrets manager.** Secrets are a binding (§7.3). Rue holds a one-time value only long enough to deliver it once.
- **Not an inventory system.** Inventory is a binding. Rue defines the contract a host record must satisfy and nothing about where it lives.
- **Not a state-machine language.** Reactive rules and control loops belong to the host (T4, §8.4). A machine decides *when* to run a plan; rue decides *how* and *how to take it back*.
- **Not a replication, detection, fencing, authority or elevation engine.** Those are ops, probes and bindings a tenant declares; rue calls them.

### 0.4 Capability, not reimplementation (the tenant test)

Rue was distilled from four projects. The recurring design mistake was to lift one of their *policies* into the engine. The rule that prevents it, applied to every mechanism in this document and to every mechanism proposed later:

> Rue provides a mechanism itself only if (a) the checker needs it in order to state a verdict, or (b) it carries a guarantee rue makes that cannot be delegated without handing the guarantee to the tenant — write-ahead journaling, reveal-once, the chain, footprint drift detection. Everything else is a **binding with a contract**, declared in the language, and the engine ships at most one generic built-in per binding kind.

| Mechanism | Verdict | Why |
|---|---|---|
| Gate *shape* (threshold grammar) | rue | The checker needs it to say "unsatisfiable" and "N humans minimum" |
| Challenge rendering, proof verification, key material, approval UX | binding | Trust and identity are the tenant's; rue supplies a request digest and consumes "authenticator X satisfied" |
| Renewal windows, TTL caps, approval windows | declared | Policy numbers are site/plan declarations; the engine has no defaults |
| Rendering a `:target` undo into a standalone artifact | rue | It is what `undo_locus: :target` *means* |
| The scheduler that runs that artifact | binding | Platform vocabulary; generic built-ins `cron()` and `task_scheduler()` only |
| Secret delivery (requester, hold, escrow) | binding | Where a credential goes is credential policy; rue guarantees once-and-nowhere-else |
| Elevation (sudo/doas/runas) | not in v0 | Rue has no elevation; `rue bootstrap` verifies and prints, never runs |
| Instance lifecycle, stuck retry, reap pass | rue | It is the write-ahead guarantee |
| Fencing, quorum, replication, cluster heartbeats, out-of-band controllers, message buses, service posture | tenant | Ops, probes and `defprim`s in `.rue` files and hooks; never in the engine |
| Journal chain and signature | rue | Tamper-evidence is a rue guarantee; *sinks* are bindings |

Every phase's exit criteria include: "each mechanism added in this phase passes the tenant test or was moved to a binding, and the seam guard is green."

### 0.5 Authority: taste, scope, and pushback

**Taste is the owner's.** Everything about how rue *looks and feels* — the Elixir-flavoured surface, the keyword names (`knell`, `wane`, `recant`), the churchyard register, the verdict prose, the CLI verbs, what a diagnostic sounds like — is decided by the project owner, who is the primary user and is building a system they enjoy using. These are not design questions and are not open for argument: an implementer who believes a spelling is unwise says so once, in one sentence, with a reason, and then implements the owner's spelling. Preferences are not bugs.

**The tenants are the floor, not the ceiling.** §8's four tenants are the minimum capability rue must have: the owner's own tooling must work. They are not the boundary of what rue is for. Rue is a public release intended for the common shop, and the common shop is not the owner's shop: the owner runs FreeBSD with Elixir and Rust; the typical adopter runs Windows or Linux with Java, Python or .NET, cron may be Task Scheduler, the console may be PowerShell; and Axonibyte's own clients run Windows infrastructure, which is the business fact behind D-031, not a preference to be weighed. Therefore a platform, language, scheduler or transport is never out of scope *because no tenant uses it*; a tenant's requirement is implemented in its general form, never its tenant-shaped one; and a capability no tenant exercises still gets a tier that proves it.

**Design pushback is welcome; scope redefinition is not.** The implementer is expected to push back, gently and with evidence, on *design*: a rule that cannot be checked, a footprint kind that does not compose, a threat the model misses, a phase whose exit criteria cannot be met. That pushback goes in one place — a "not proven" table or §11 — and waits for the owner's answer. The implementer does not narrow the platform or language set; drop or defer a tenant's requirement; rename or "improve" surface vocabulary; reopen a decision inside a phase (a superseding index entry approved by the owner is the only way a decision changes); or treat the tenants as the definition of "done."

When in doubt: the owner decides what rue is *for* and how it *feels*; the implementer decides how it is *built*, within this document, and asks when the document is silent.

---

## 1. The claim and its prior art

### 1.1 The claim

> rue is the first language for operations against hosts in which "this can be undone" is a compile-time verdict rather than a comment — computed from declared footprints, undo loci and refusal modes rather than by search over a world model, and stating where the undo runs, past which step it cannot, what that step costs and who must acknowledge it, and how long the plan is bounded, as prose and as a stable structured form.

The claim is narrow on purpose. It is tested by trying to falsify it; the prior-art table is the falsification attempt, maintained as part of the README. Deciding undoability offline is not new (action reversibility in AI planning; the compensation calculi); deciding it for a plan language from declarations, with locus and cost in the answer, is. The 2026-09-06 sweep that narrowed the sentence to this form is `docs/prior-art.md`.

### 1.2 Prior art, and the delta from each

| Prior art | What it has | What rue adds |
|---|---|---|
| Action reversibility in AI planning (Eiter, Erdem & Faber 2008; Morak, Chrpa, Faber & Fišer, KR 2020; Med et al. 2024, 2025) | Decides offline whether an action's effects can be undone, by search over a STRIPS-like domain; PSPACE-hard in general | Decides from declarations, not search; the verdict states locus, cost, acknowledgement, arming order and bound, none of which the planning model has |
| Compensation calculi (Bruni, Melgratti & Montanari, POPL 2005; Sagas calculi; compensating CSP) | Semantics and expressiveness of compensations; decidability with static compensations | No footprints; no check that a given program's compensations compose; rue is the checker the calculi lack |
| Sagas / compensating transactions (Garcia-Molina & Salem, 1987) | Sequenced steps with hand-written compensations | The compensations are typed, checked for composition, and their locus is known |
| Temporal / Cadence | Durable execution; saga pattern for compensation | Checks nothing about compensations; no undo that survives the engine's death; no point of no return |
| Junos `commit confirmed` | Apply, auto-revert unless confirmed, on one device | Generalised to any op with a target-standalone undo; the `reach` rule proves the arming order |
| Database migrations (up/down) | Paired inverses | Unchecked; rue verifies inverse composition and footprint disjointness |
| NixOS generations | Atomic rollback of OS config | Restorative undo for one footprint kind only; no ops, no ordering, no cost |
| Terraform / Kubernetes | Declarative convergence | Convergence, not reversal; no concept of an irreversible step |
| Janus, reversible computing | Language-level reversibility | No side effects on a world; no footprints, no locus |
| Lenses / bidirectional transformations | Checked inverses over data | Data, not operations against hosts |
| Ansible `when:` / `block`/`rescue` | Conditional steps, rescue blocks | No checker; conflicts discovered at runtime on the host |
| Ecto.Multi | Named steps, all-or-nothing within a transaction | Inside one database; no partial reversibility, no locus |
| Miniscript / Antelope authorities | Threshold-and-timelock policy with load-time satisfiability | Policy, not operations; the model rue borrows for gates |
| Metafont / Dhall | Total, deterministic languages | The totality ethic rue adopts; neither is about effects |

### 1.3 Falsification standing order

Before Phase 0 exits, and again before any public release, one person spends one day trying to name a system that makes reversibility a checked property across heterogeneous operations. If one is found, the claim is narrowed in §1.1 and the delta added to §1.2. Done once, 2026-09-06 (`docs/prior-art.md`): two were found and the claim was narrowed as above.

---

## 2. Vocabulary

| Term | Meaning |
|---|---|
| **rue** | The language, and the client CLI |
| **rue-core** | The pure library: model, checker, journal model, verdict rendering. No I/O, no clock |
| **engine** | The executor: runs plans through bindings, journals, arms backstops. Standalone daemon `rued`, or embedded via hooks |
| **fact** | A typed observation of the world, produced by a probe. Three-valued: `value \| :unknown` |
| **probe** | A declared way of observing a fact. The fact SDK; the engine ships a generic minimum, tenants declare the rest |
| **op** | The unit of change: footprint, pre, do, undo, post, undo locus, refusal mode, drift policy, reach, cost, outputs |
| **plan** | A total sequence of steps over ops, with `par` blocks, gates, a backstop, an intent, and at most one `knell` per point of no return |
| **step** | One op instantiated in a plan with bound parameters and a locus |
| **intent** | `:temporary` (has `wane`, reverts at expiry) or `:permanent` (ends with `commit()`, keeps its changes). Inferred, never both |
| **footprint** | The set of facts an op may change, each with a **kind**, a static **shape** and a dynamic **instance** |
| **umbra** / **penumbra** | The facts an op *definitely* touches / *may* touch because the footprint shape is bound only at runtime. Penumbra conflicts are *may-conflict* |
| **undo locus** | Where an op's undo can run: `:target` (renderable to a standalone artifact on the host), `:controller` (needs the engine alive), `:none` (irreversible) |
| **refusal mode** | What a step does when a later step refuses: `:revert`, `:hold`, or (for `knell`) nothing |
| **drift policy** | An op's `drift: :clobber \| :defer`: what its undo does when the fact changed since `do`, applied identically by the engine and a standalone artifact |
| **knell** | A step declared irreversible: the point of no return. Has a guard, a cost probe, and an acknowledgement |
| **wane** | A temporary plan's time-to-live. When it elapses, the armed undo fires. Permanent plans have none |
| **commit** | The item that ends a permanent plan (or the verb, from `Held`/`Deferred`): undo discarded, umbras released |
| **backstop** | The mechanism that fires an undo without the engine: a rendered artifact on the target, armed with a **trigger** |
| **trigger** | What fires a backstop: `after:` (temporary plans, equal to `wane`), `unless_confirmed:` (permanent plans, or temporary via `fires_by_construction`), `unless_heartbeat:` (dead-man; either intent) |
| **confirm** | The step that disarms an `unless_confirmed` backstop |
| **recant** | The verb that runs a plan's undo |
| **reach** | An op's declaration that its footprint includes the transport the engine uses to reach the host |
| **gate** | An approval requirement on plan entry or on a step, satisfied through the approval binding |
| **hold** | A refusal disposition: leave the world as-is, print the undo, wait for a human. May require a mutation (`hold_via:`) |
| **deferred** | A step whose locus is a host the running engine cannot act on; the handoff command is printed |
| **stuck** | A runtime outcome: an undo failed; persisted, retried every pass, never swallowed |
| **DriftHeld** | An instance state: an undo declined by a `:defer` policy; holds its umbras until forced or abandoned. Not `Stuck` |
| **Waiting** | An instance state: a partially applied plan paused at a step gate, a knell acknowledgement, or an `:unknown` guard |
| **guard** | A `when` condition over facts. Three-valued: `:yes \| :no \| :unknown`. Fires only on `:yes` |
| **force** | An operator override that applies only to `:unknown` guard results or to the runtime classes `drift` and `unknown`. Some guards declare `force: never` |
| **verdict** | The output of `check`: prose plus a stable structured form |
| **journal** | The append-only, hash-chained record of intent and outcome |
| **binding** | A declared external: inventory, journal, approval, secrets, notify, executor, scheduler. Every binding has a **contract** and a **kind** |
| **hook** | A binding kind whose implementation the host registers by name before load. The embedding primitive |
| **registrar** | A declared identity permitted to register named hooks |
| **operator** | A declared identity permitted to act on the control channel; **admin** operators may run non-plan verbs |
| **control client** | Any process on the control channel with a declared operator identity: the CLI, or an embedded host acting as operator |
| **tenant** | A project that consumes rue. Its integration surface is `.rue` files plus hook registrations. No tenant name appears in framework code |
| **site** | The block of a `.rue` program that declares bindings and identities. Loaded before any plan is checked |
| **instance directory** | `<rue_root>/instances/<id>/` on every run-capable host an instance touches: shim, staged files, snapshots, markers, region manifest, and the backstop artifact if any |
| **controller-side markers** | Markers, snapshots and manifests kept in the controller's store for hosts with no run-capable executor |
| **stage** | A body primitive that writes a mode-0600 file into the instance directory for one step, then removes it |
| **stdin preamble** | How secrets reach a remote `run`: NUL-separated pairs read from stdin by a rue shim on the target before exec; never argv |
| **reclaim** | Removing an orphaned instance directory; refused while its artifact is armed with a present scheduler unless `--force --reason` |
| **abandon** | Admin verb closing a `Stuck` or `DriftHeld` instance with the world left as-is; journaled with what was not reverted |
| **explain** | The per-host rendering of a plan as ordered steps, each with its undo line, locus, refusal mode, drift policy and gates |
| **render** | The rendering of a plan's expected end-state facts, never touching a host; sourced from `post` guards of the form `fact == value` |
| **observe** / **assert** | Probe-only items: `observe` binds a probe's outputs with no footprint; `assert` refuses or waits on a guard |
| **defprim** | A tenant-declared body primitive: a `run` template with declared argument classes |
| **daemon dry-run** / **request dry-run** | `rued --dry-run`: no executors, no operators required. `rue apply --dry-run`: a journal-only rehearsal against a real daemon that reserves nothing |

Reserved, not used in v0: **shrive** (a checked plan is *shriven*), **sexton** (a possible daemon name; `rued` is the working name).

---

## 3. Decisions index

Each decision is a constraint on every phase. The rule lives in the section named; this index gives the reason. Reopening a decision is a roadmap change recorded as a superseding entry approved by the owner.

| # | Decision | Section | Rationale |
|---|---|---|---|
| D-001 | Standalone language in Rust, Elixir-flavoured surface, no BEAM | §6.1 | Totality and the checker are the product |
| D-002 | The surface is total: no closures, recursion, or unbounded loops | §6.1 | Footprints must be computable at check time |
| D-003 | Types are invisible; the checker speaks in prose and a structured form | §6.5 | 3am readability |
| D-004 | Reuse is pipelines, modules and pattern-matched clauses | §6.4 | Elixir feel, each with a totality rule |
| D-005 | Values flow between ops; values are data, never functions | §6.5 | Footprints get a static shape and a dynamic instance |
| D-006 | Refusal mode is declared per op and composes into the verdict | §5.5 | Different steps have different right answers |
| D-007 | Facts come from a probe SDK | §5.1 | Footprint honesty rests on reviewable declarations |
| D-008 | Execution locus is per op; reachability enters the verdict | §5.3 | Where an undo runs is part of the guarantee |
| D-009 | `par` admitted only when umbras are provably disjoint | §5.7 | Same query as the interference check |
| D-010 | Strict by default; may-conflict is a failed check unless the plan says `:warn` | §5.7 | Fail closed |
| D-011 | Footprints have kinds; undo strategy is a function of kind | §5.2 | Restore-from-snapshot would clobber other actors' edits |
| D-012 | Every op has an undo locus; a `:target` undo is renderable and closed | §5.3 | Fail-closed expiry needs an undo that runs with the engine dead |
| D-013 | Backstops have triggers and are ops that participate in ordering | §5.6 | Commit-confirmed and dead-man are one mechanism |
| D-014 | The `reach` rule: a `:target` undo, backstop armed before the op | §5.6 | The classic self-lockout, proven by the checker |
| D-015 | Guards are three-valued; force applies only to `:unknown` | §5.1, §6.3 | Detection cannot tell dead from isolated |
| D-016 | `knell` steps have a guard, a measured cost and an acknowledgement | §5.3, §5.11 | Irreversible with a price and a recorded decision |
| D-017 | Stuck is a first-class state; apply is atomic-or-reported | §5.9 | A failed open never presents as cleanly open |
| D-018 | Plan entry may be gated; proofs accumulate; `wane` anchors at approval | §5.11 | The human authorises, the plan runs inside the grant |
| D-019 | One plan instance per (host, exclusivity class); a second refuses with 75 | §5.12 | Refuse, never queue |
| D-020 | The engine has no opinions: every external is a declared binding | §7.3 | The language dictates; the engine executes only what it declares |
| D-021 | `hook` is a binding kind, the embedding primitive | §7.5 | One concept across journal, inventory, notify, probes and ops |
| D-022 | The journal is hash-chained by the engine before any sink; delivery is write-ahead | §5.10, §7.6 | A hook can drop entries detectably, never forge them |
| D-023 | The verdict's structured form is a compatibility surface | §5.8 | Declared tooling consumes verdicts |
| D-024 | No tenant vocabulary in the framework; a seam guard enforces it | §4.4 | reaper's tenant rule |
| D-025 | Secrets: once, never journaled, never argv, delivered by binding | §5.13, §7.10 | One-time credentials are information effects |
| D-026 | Dry-run and settle are engine features | §7.9 | Learn before acting |
| D-027 | Diagnostics are a golden-tested compatibility surface | §6.7 | isolex's rule |
| D-028 | The name is `rue`; `.rue`, `rued`, `recant`, `knell`, `wane` | §2 | Chosen |
| D-029 | Phase 0 is a typed-host prototype before any Rust | §9 | The theory is unproven |
| D-030 | The four tenants are the grading rubric | §8 | Tenants, not features |
| D-031 | Windows is first-class for the whole toolchain | §4.5, §0.5 | Axonibyte's clients run Windows infrastructure; a platform nothing exercises is broken |
| D-032 | rue is a reaper tenant from Phase 1 | §12 | The harness exists |
| D-033 | Bitbucket primary, mirror step first, tag-gated deploy | §12 | House pattern |
| D-034 | Embedding SDKs are thin, conformance-tested clients of the hook protocol; the initial set is Rust, Elixir, Python, Java, .NET and the shim | §7.11 | Language-agnostic by construction |
| D-035 | Pinned toolchains; `rustls` mandatory | §12 | Reproducible builds |
| D-036 | Rue owns the gate *shape*; the binding owns proofs and identity | §5.11 | Policy mistakes surface at check time |
| D-037 | A compromised target defeating its own backstop is accepted in v0 | §7.12 | The shipped guarantee is "reverts if the controller dies" |
| D-038 | Secret delivery is an ordered list of acceptors | §7.10 | Where a credential goes is credential policy |
| D-039 | The tenant test (§0.4) binds every mechanism | §0.4 | Capability, not reimplementation |
| D-040 | Taste is the owner's; tenants are the floor | §0.5 | The owner is the primary user of a public release |
| D-041 | Drift under a standalone undo follows the step's policy, decidable without a human | §5.2, §7.7 | The artifact has no operator |
| D-042 | Secrets never touch argv or target disk in clear | §5.13 | `ps` and shell history |
| D-043 | The request digest covers the host contract | §5.11 | Approval binds to what will actually run |
| D-044 | Install and arm are separate | §5.6 | Markers need the directory to exist |
| D-045 | `repeat` iterations are distinct by construction; `when` has `else` | §6.3, §5.7 | A per-item loop must check clean |
| D-046 | Zero-human gates refuse by default | §5.11 | A wait alone is a delay, not a control |
| D-047 | Drift policy is per step and executor-agnostic | §5.2 | The world ends in the same place whoever ran the undo |
| D-048 | Region undo with damaged markers: whole-fact restore, conditionally | §5.2 | The answer must be defined, not discovered |
| D-049 | Mid-plan waits are states; step gates use a step digest | §5.9, §5.11 | A paused plan is a state the store must survive |
| D-050 | Secrets reach remote `run`s only through the stdin preamble | §5.13, §7.4 | The hook boundary must not make embedded tenants secretless |
| D-051 | `stage()` files are footprint and are recovered | §5.13, §7.7 | A crash must not leave a secret on disk |
| D-052 | Region disjointness is by (fact, anchor); `when` on `:unknown` waits | §5.2, §6.3 | Multiple plans on one host must coexist |
| D-053 | Whole-fact restore only when no other instance holds a region on the fact | §5.2 | Rue's own artifact may not violate rue's footprint rule |
| D-054 | Every instance has a directory on every run-capable host it touches | §7.7 | Secrets, snapshots and markers need a home |
| D-055 | Step gates have an approval path; ack is a scope | §5.11, §6.8 | A human must be able to satisfy what the language lets you declare |
| D-056 | What each state holds | §5.9 | Holding is the safe direction; the cost is visible |
| D-057 | Embedded hosts are operators, not automation | §7.5 | A program that can decide is an operator |
| D-058 | A knell acknowledgement is a step gate in the `ack` scope; the requester may ack | §5.11 | One machinery for approvals |
| D-059 | Boot reconciliation never disarms a live backstop | §7.7 | The confused controller is when the backstop matters |
| D-060 | Instance directories only on run-capable hosts | §7.7 | A BMC has no shell |
| D-061 | An embedded host is a control client, not a privileged hook | §7.4 | One privileged channel, one identity model |
| D-062 | `Pending` reserves umbras; rehearsals reserve nothing | §5.12 | Refuse before collecting proofs |
| D-063 | (folded into D-058) | §5.11 | — |
| D-064 | Waits lapse into the step's refusal per `on_lapse` | §5.9 | A wait that outlives its window is a refusal |
| D-065 | The foreign-region condition is per step | §5.2 | Same rule whoever runs the undo |
| D-066 | Manifest access takes a host-wide lock, held across the region undo | §7.7 | The decision must be atomic with its write |
| D-067 | Control-channel identity: peer credentials mapped to declared operators | §7.4 | D-061 needs an identity to scope |
| D-068 | Waits are bounded | §5.9 | An unbounded wait is a lock nobody releases |
| D-069 | Step evaluation order | §5.4 | One sentence settles every ordering question |
| D-070 | (folded into D-066) | §7.7 | — |
| D-071 | Reclaim has a bounded override | §7.7 | An artifact whose scheduler is gone must not be unreclaimable |
| D-072 | No implicit operators | §7.4 | Group membership grants a connection, never an identity |
| D-073 | Instance directory ownership and the target bootstrap | §7.7 | Two users share the directory; someone creates it first |
| D-074 | Every wait has one effective bound; `wane` always wins | §5.9 | The artifact's deadline is the engine's deadline |
| D-075 | A `:target` undo with no filesystem at runtime refuses before `do` | §5.4 | Refuse before, not stick after |
| D-076 | Hook registration has the same identity model as operating | §7.4 | Trusting a hook by name means declaring who may bear it |
| D-077 | Shared files are replaced by rename, never rewritten | §7.7 | Two users, one directory, no in-place writes |
| D-078 | `DriftHeld` is exempt from `wane` | §5.9 | The alternative is a policy override nobody asked for |
| D-079 | In a temporary plan `wane` is required by state reachability | §5.9 | A bound that only applies when declared is not a bound |
| D-080 | Bootstrap is the tenant's provisioning; rue verifies and prints | §7.7 | A verb claiming rights the design lacks is a lie |
| D-081 | One socket, `hello` then role; non-plan verbs need `admin: true` | §7.4 | Scope for plans, admin for the machinery |
| D-082 | Two dry-runs, named distinctly | §7.9 | Two things sharing a name get confused at 03:00 |
| D-083 | Identity events are journaled | §5.10 | Trust by name is only auditable if the bearer is on record |
| D-084 | Admin is the most powerful identity, on the record; `rued migrate` is exempt | §7.4, §7.12 | The loudest journal for the strongest identity |
| D-085 | A rehearsal evaluates gates without awaiting proofs and reserves nothing | §7.9 | A rehearsal that blocks the real thing is not a rehearsal |
| D-086 | `:hold` under `mode: :auto` is not refused; `Stuck` is an unbounded state | §5.9 | Holding-then-deciding is a legitimate outcome |
| D-087 | Plans have an intent, inferred; permanent plans end with `commit()` | §5.4 | An upgrade that reverts on a timer is not an upgrade |
| D-088 | Bounds follow intent; `Held`/`Deferred` in a permanent plan are unbounded | §5.9 | A held permanent change waits for a decision, not a timer |
| D-089 | Backstop triggers follow intent; the dead-man belongs to both | §5.6 | Temporary changes expire; permanent changes are confirmed |
| D-090 | Every instance can be ended by a human: resume, handoff-done, abandon | §5.9, §6.8 | An instance nobody can end is a lock nobody can release |
| D-091 | State transitions are stated by class; secrets deliver at the producing step | §5.9, §5.13 | The machine is derived from the rules |

---
## 4. Architecture

### 4.1 Layers, strictly

```
SURFACE     .rue files: site block, defprobe/defprim/defop/defplan/defrole/defprotocol   DATA
    v
FRONT END   lexer, parser, lossless tree, resolver, clause dispatch, template expansion,  CODE (rue-surface)
            diagnostics
    v
CORE        model (incl. Body and Prim), footprint algebra, refusal lattice, checker,     CODE (rue-core, pure)
            verdict, journal model, state machine
    v
RENDER      artifact text per OS family, quoting, closure-check inputs                   CODE (rue-render, pure)
    v
ENGINE      bindings, executors, control channel, hook protocol, journal sinks, arming,   CODE (rue-engine)
            write-ahead lifecycle, reconciliation, dry-run, settle
    v
BINDINGS    generic built-ins only (file, local, ssh, rue_toml, stdout, cron,             CODE (rue-bindings)
            task_scheduler, requester, hold, always)
            + tenant hooks, probes, defprims                                              TENANT
```

If a topic string, a device name, a platform verb or a house rule appears in a CODE row, the layering has slipped and the seam guard (§4.4) should have caught it.

### 4.2 Crate layout

```
rue/
  Cargo.toml                  workspace
  core/          rue-core     pure: model, checker, verdict, journal model, state machine
  render/        rue-render   pure: artifact rendering per OS family, quoting; depends only on core
  surface/       rue-surface  lexer, parser, tree, resolver, diagnostics
  hook-proto/    rue-hook-proto  pure: the hook protocol's wire as data -- the ops of 7.5,
                              the registration frame, the resolved body, the reply records
  engine/        rue-engine   lifecycle, bindings API, control channel, hook protocol, arming
  bindings/      rue-bindings generic built-ins only
  cli/           rue          the operator CLI (FreeBSD, Linux, Windows)
  daemon/        rued         the standalone engine daemon (FreeBSD, Linux, Windows)
  sdk/           hook-protocol clients over rue-hook-proto: rust/ (reference), elixir/,
                 python/, java/, dotnet/, shim/ (rue-hook)
  tenants/       acceptance tenants: .rue files + hook stubs + expected verdicts
  sim/           the tier-7 shadow world
  proto/         Phase 0 prototype (Haskell or OCaml), kept as a record
  docs/          DESIGN.md, LANGUAGE.md, TESTING.md, this file, verdict-schema.json,
                 hook-protocol.md, control-protocol.md, prior-art.md
  tools/         check.sh, lint-seam.sh, seam-denylist.txt, rediscovery/
  ci/            build-target.sh
  tree-sitter-rue/            Phase 5
  .reaper.toml   bitbucket-pipelines.yml
```

Dependency direction is downward only. `rue-core` depends on nothing in the workspace and on no I/O crate; `Body`, `Prim` and each primitive's argument classes are core types so closure and secret-placement analysis live in core. `rue-render` depends only on core and performs no I/O. `rue-surface` depends on core. `rue-engine` depends on all three. `rue-bindings` depends on `rue-engine`. The CLI and daemon depend on everything. Tenants depend on the public API only.

### 4.3 Trust boundary

- `rue-core` trusts nothing about the world. It reasons only over declared footprints, loci and modes. Its theorem is: *given honest declarations, the verdict is correct.*
- `rue-engine` enforces honesty at runtime: after every `do` it diffs observed facts against the declared footprint; drift *outside* the footprint aborts the plan (the reversibility proof no longer holds); drift *inside* the footprint at undo time is another actor, handled by the step's drift policy (§5.2).
- Probe declarations, hook implementations and registrar/operator declarations are trusted by the engine and are the reviewable surface. The README says this on the first screen.

### 4.4 The seam (tenant guard)

`tools/lint-seam.sh` greps every non-tenant crate of the workspace -- the directories its `SCAN` line names -- for a denylist in `tools/seam-denylist.txt` and fails on any hit; `tests/tier3/t_seam.sh` binds that line to the workspace's member list, so a crate added in a later phase cannot fall outside the guard silently. The denylist starts with the four tenants' names and platform vocabulary and grows by one line every time a tenant is onboarded. It runs in `tools/check.sh` and in CI. `tests/tier3/t_seam.sh` asserts the guard itself fails when a denylisted word is planted.

### 4.5 Platforms

| Component | FreeBSD | Linux | Windows | macOS |
|---|---|---|---|---|
| `rue` CLI | yes | yes | yes | cross-built; untested until a Mac exists (§11) |
| `rued` engine | yes (rc.d) | yes (systemd) | yes (Windows service) | cross-built; launchd daemon untested |
| Generic executors | `local()`, `ssh()` | `local()`, `ssh()` | `local()`, `ssh()` (OpenSSH for Windows) | `local()`, `ssh()` |
| Control channel | unix socket 0660 group `rue`, peer-credential identity | same | named pipe, DACL for group `rue`, client-SID identity | unix socket, `getpeereid` |
| Backstop artifact | POSIX `sh` or Python; scheduler `cron()` | POSIX `sh` or Python; scheduler `cron()` | PowerShell or Python; scheduler `task_scheduler()` | POSIX `sh` or Python; scheduler `launchd()` (Phase 3; cron is deprecated and TCC-bound there) |
| Controller store | `/var/db/rue` | `/var/db/rue` | `%ProgramData%\rue` | `/var/db/rue` |
| Target `rue_root` (bootstrapped once, §7.7) | `/var/db/rue` root:`rue`; `instances/` 2770; `lock` 0664 | same | `%ProgramData%\rue` with group DACL | same as FreeBSD |
| Build target | `x86_64-`/`aarch64-unknown-freebsd` | `x86_64-`/`aarch64-unknown-linux-gnu` | `x86_64-pc-windows-gnu` (`-msvc` if the service wrapper needs it) | `x86_64-`/`aarch64-apple-darwin` |

Every OS renders both its native shell and Python as artifact languages, per host (`HostRecord.artifact`; the native shell when absent): Python is a standard-library-only script run by `uv run --offline --script` with PEP 723 inline metadata on every OS, so a site whose Windows hosts cannot run PowerShell (execution policy, AppLocker, WDAC) standardizes on one artifact language across its OS mix; `uv` with a cached interpreter is an arm-time precondition wherever a host declares Python (§11).

Per-OS knowledge lives in exactly three places: `rue-render` (templates and quoting), the OS family's generic executor and scheduler bindings in `rue-bindings`, and `ci/build-target.sh`; the one stated exception is `rue-core`'s artifact vocabulary (which shell family an `os` implies, which languages have a template), there because the checker refuses at check time and core cannot depend on render. The OS family of a host comes from `HostRecord.os`; a `:target` backstop on a host whose declared language has no template for its OS is E0403.

---

## 5. Core model (rue-core)

Names are normative.

### 5.1 Facts and probes

```
Fact     = { path: FactPath, value: Value | Unknown, observed_at: Instant, stale_after: Option<Duration> }
FactPath = [Segment]                        -- dotted in source: host.db01.service.posture
Value    = Str | Num | Bool | Atom | Duration | List<Value> | Record<Map<Str,Value>> | Secret<Value>
Probe    = { id, produces: [FactShape], locus: Target | Controller, equivalence: Equivalence, static: bool }
Tri      = Yes | No | Unknown
```

- A fact older than `stale_after` reads `Unknown`. Every operator over values propagates `Unknown`. `defined?(x)` is the only way to ask whether a fact is known; comparing against `:unknown` is E0108.
- A guard is an expression yielding `Tri`. It fires only on `Yes`. `force:` on a step applies only to `Unknown` results of the named guards; some guards declare `force: never`. A plan in `mode: :auto` admits no `force:` (E0404), checked statically.
- `Secret<V>` is a value whose rendering is redacted everywhere except the single delivery (§5.13). Any operator over a `Secret` yields a `Secret`.
- Facts split into **host-contract facts** (the `HostRecord` fields and any probe declared `static: true`, frozen at request and used for clause dispatch) and **probed facts** (observed at point of use, used by guards). A clause pattern may reference only the former (E0111); a guard may reference both.
- A probe's `equivalence` (`:bytes` default, `:line_set`, `:json`, or tenant-declared) is the notion of "restored" for the facts it produces.

### 5.2 Footprints

```
Footprint      = [FootprintEntry]
FootprintEntry = { kind: Kind, shape: FactShape, instance: Option<FactPath>, anchor: Option<Expr> }
Kind           = Owned | Region | Modified | Derived | AppendOnly | Held
```

**Shape** is static (known at check time); **instance** is bound at runtime when a value flows in. Interference and disjointness are computed on shapes; overlap of shapes with unbound instances is *may-conflict* (penumbra). Iterations of `repeat over:` bind distinct instances of the same shape and are pairwise disjoint by construction (the list must be set-valued, E0113).

| Kind | Meaning | Undo strategy | Drift at undo (engine **and** artifact, identically) | Default `drift:` |
|---|---|---|---|---|
| `Owned` | The op creates the fact and owns it wholly | delete | `:clobber` removes regardless of content; `:defer` leaves, marks | `:clobber` |
| `Region` | A fenced region inside a fact others may edit | strip between markers | Markers intact: strip regardless of content. Markers damaged: `:clobber` restores the whole fact from the do-time snapshot **only if no other active instance holds a region on that fact**, otherwise defers; `:defer` leaves, marks | `:clobber` |
| `Modified` | The op changes an existing fact | restore from snapshot | Fact equals its post-`do` value: restore. Otherwise `:clobber` restores anyway; `:defer` leaves, marks | `:defer` |
| `Derived` | Read-only; verified, never written | none | n/a | n/a |
| `AppendOnly` | A record only ever appended | compensating append | n/a; `explain` says "undone by record, not erasure" | n/a |
| `Held` | A controller-held ephemeral resource (a process, a tunnel, a lease) | release | n/a; has `suspend`/`reestablish`; reestablish never re-runs a secret-producing `do` | n/a |

Rules:

- **Drift policy is per step and executor-agnostic.** `:clobber` journals `DriftClobbered{step, facts}`; `:defer` records a drift marker and the instance enters `DriftHeld` (§5.9). A `reach` op whose undo is `:defer` is E0410: a lockout that "reverts unaided" must actually revert. `explain` prints the policy beside every step, and the cost of a `region` op's damaged-marker fallback (a stranger's edits outside the region are lost).
- **The foreign-region condition is per step.** Engine and artifact both defer a region's whole-fact fallback when another active instance holds a region on the fact; the engine knows from the cross-plan ledger, the artifact from sibling instance directories' manifests (§7.7). The verdict marks such steps conditional.
- **Region disjointness is by (fact, anchor).** Distinct anchors on one fact are disjoint umbras; the same anchor twice in one plan is E0305. A backstop's scheduler region uses the instance id as its anchor; a tenant op's anchor is a literal or an expression the checker can prove distinct across instances (otherwise may-conflict). After a permanent plan commits, its regions become ordinary content with no ledger owner; a later plan may claim the same anchor.
- **Never destroy what is not in your footprint.** Any `do` or `undo` observed touching a fact outside the declared footprint aborts the plan (R0201). Files created by `stage()` are auto-added to the step's `Owned` footprint.

### 5.3 Ops

```
Op = {
  id, params: [Param],
  footprint: Footprint,
  pre:  [Guard],                 -- must be Yes to run; Unknown refuses (the op author's contract, not a decision point)
  do:   Body,
  undo: Restore | Computed { body, undo_pre: [Guard] } | Compensate { body, undo_pre } | None,
  post: [Guard],                 -- verified after do; failure => step failed => atomic-or-reported
  undo_locus: Target | Controller | None,
  refusal: Revert | Hold { via: Option<OpRef> } | Knell { guard: Option<Guard>, cost: Probe | None{reason}, ack: GateSpec | None{reason} },
  drift: Clobber | Defer,        -- default by footprint kind
  reach: [Transport],            -- transports this op may sever
  outputs: [{ name, secret: bool }],
  exclusivity: Option<Class>,
  locus: Controller | Target | Host(Expr),
  suspend: Option<Body>, reestablish: Option<Body>,   -- required iff a Held footprint
  handoff_done: Option<Probe>    -- continues a Deferred instance when it reads Yes
}
```

- `undo: None` ⇔ `refusal: Knell` (E0201). `undo_locus: None` ⇒ `undo: None` (E0203).
- `undo_locus: Target` ⇒ the undo body is *closed*: it references only target-local commands and facts observable on the target (E0202), and no `Secret` (E0210).
- `Restore` derives `undo_pre` automatically (each modified fact equals its post-`do` value); `Computed`/`Compensate` MUST declare `undo_pre` (E0207). Every undo MUST be idempotent (E0208): a rerun after a partial run, or after the backstop already fired, succeeds and changes nothing.
- `Knell` ⇒ a **guard** (a `Tri` that must be `Yes`; `Unknown` refuses under `mode: :auto`, waits otherwise) is optional; a **cost** (a probe returning a value the acknowledger sees, never a `Tri`) is required or declared `:none` with a reason (E0204); an **ack** gate is required or declared `:none` with a reason (§5.11).
- A `Held` footprint ⇒ `suspend` and `reestablish` defined (E0205). A `Secret` output ⇒ the op is not reachable from `reestablish` (E0206).
- `reach` non-empty ⇒ `undo_locus: Target` and a backstop armed before the step (§5.6, E0401).
- A body may take secrets only via `env:`/`stdin:` on `run` or via `stage()`; a `Secret` interpolated into a `run` string is E0209.

### 5.4 Plans, items, intent, and step order

```
Plan = {
  id, params, host_pattern: Pattern,
  intent: Temporary | Permanent,          -- inferred
  gate: Option<{ spec: GateSpec, window: Option<Duration>, allow_zero_human: bool }>,
  wane: Option<Duration>,                  -- temporary plans only; anchored at approval
  renew_within: Option<Duration>,
  backstop: Option<{ triggers: [Trigger], locus: Target, arm_before: StepRef }>,
  fires_by_construction: bool,
  strictness: Strict | Warn,
  mode: Manual | Auto,
  exclusivity: Option<Class>,
  require_journal: Option<Chained | Signed>,
  body: [Item]
}
Item = Step { op, args, locus, gate: Option<GateSpec>, window: Option<Duration>, on_lapse: Revert | Hold, force: [ForceName], alias: Option<Name> }
     | Par [Item]
     | Slot(Name)
     | Knell(Step)
     | Confirm | Commit
     | Preflight [Guard]
     | Observe { probe, alias }
     | Assert { guard, window: Option<Duration>, on_lapse: Revert | Hold }
     | Repeat { form: Count(Int) | Over { list: Expr, max: Int }, var, body: [Item] }
     | When { guard, window: Option<Duration>, on_lapse: Revert | Hold, then_: [Item], else_: [Item] }
ForceName = Guard(GuardRef) | Drift | Unknown
Trigger   = After(Duration) | UnlessConfirmed(Duration) | UnlessHeartbeat { deadline: Duration, interval: Duration }
```

**Intent.** A plan with `wane` is temporary; a plan with a reachable `commit()` is permanent; both or neither is E0501, except a plan declaring `fires_by_construction: true`, which is temporary with its `unless_confirmed` duration as `wane`. A permanent plan MUST reach `commit()` on every non-refusing path (E0505), and `commit()` MUST be the last item on its path (E0502). Committing discards the undo, releases umbras and exclusivity, removes instance directories, disarms every backstop first, and journals `Committed{by, reason}`.

**Expansion.** Templates (`defop`, `defplan`, `defrole`, `defprotocol`) are expanded before checking; clauses are dispatched per inventory host; slots are filled from roles in declared priority order (ties: role definition order, then contribution order); the checker sees one concrete plan per host.

**Step evaluation order.** `pre` → step `gate:` (may enter `Waiting{gate}`) → for a knell: `guard` (may refuse or `Waiting{unknown_guard}`) → `cost` probe → `ack` (may enter `Waiting{ack}`; the acknowledger is shown the cost) → pre-step locus check (a `:target` undo on a host whose executor reports no filesystem refuses here, R0408, and the applied prefix reverts) → footprint snapshot → write-ahead journal entry acked → `do` → `post` → completion marker → commit. Only `gate`, `guard`, `ack`, `assert` and `when` can produce `Waiting`; `pre` and `post` refuse. `Preflight` runs every listed guard before any mutation and again at point of use. `Confirm` disarms an `unless_confirmed` backstop. `Observe` binds outputs with no footprint. `Assert` refuses on `No` and waits on `Unknown`. `When` on `Unknown` runs neither arm and waits; `--force=unknown` selects the `else` arm; under `mode: :auto` an `Unknown` `when` refuses. Both arms' footprints count for interference.

**Value flow.** A step bound with `as name` exposes its op's outputs as `name.<output>` to every later item in the same or an enclosing block. Outputs of `par` children are visible after the `par`; outputs inside `repeat` are visible only within that iteration; a `when` block's outputs are `Unknown` outside it unless both arms bind the same name with the same kind (E0114). Referencing an output before its step, or from a sibling `par` child, is E0110. A step's locus may be `host(expr)` bound from an earlier output; the verdict then lists it as unresolved.

### 5.5 The refusal lattice and composition

```
Revert < Hold < Knell        (a plan's mode up to each knell is the join of its steps' modes)
```

For a plan `[s1 .. sn]`:

- **Reversible through k**: for every knell-free prefix `[s1..sk]`, running `undo(sk) .. undo(s1)` restores every umbra fact to its pre-plan value, because no later step's umbra overlaps an earlier step's in a way that falsifies the earlier undo's precondition (§5.7). The verdict's `reversible_through` is the step before the first knell when there is one (steps that only observe are reversible trivially), else the last mutating step: `confirm()`, `commit()` and observations after it change nothing and do not extend it.
- **Holding at k**: the first step with `refusal: Hold` in each knell segment — the items before the first knell, and between consecutive knells. A refusal after it leaves the world as-is from k on. The verdict's `holds_at` lists one step per segment; `held_indefinitely` lists every `:hold` step of a permanent plan and every deferred step (§5.9 rule 3).
- **Point of no return at k**: the first knell. Everything before it is reversible up to it; everything after is reversible back to it.
- **Partial reversal is the real operation.** `reverse_from(k)` undoes the applied prefix last-in-first-out. `reverse` is `reverse_from(n)`.
- `par` blocks are reversible iff every child is, with undos run in parallel; admitted iff children's umbras are pairwise disjoint (E0303); `reach` inside `par` is refused (E0304).
- Laws the Phase 0 prototype must exhibit: `reverse(seq a b) = seq (reverse b) (reverse a)`; `reverse(par xs) = par (map reverse xs)`; `reverse(knell) = refuse`; `reverse . reverse = id` on knell-free plans.

### 5.6 Backstops and ordering

A backstop is an op: its footprint is the artifact file inside the instance directory plus a `Region` in the scheduler anchored by instance id; it is checked for interference like any other op. Its rendering and installation are §7.7.

- **Triggers follow intent.** A temporary plan's backstop is `after:` equal to its `wane` (E0503 otherwise) and MAY add `unless_heartbeat:`; a `fires_by_construction` plan is the one temporary plan whose expiry is `unless_confirmed:` instead of `after:`, its duration serving as `wane`. A permanent plan's backstop is `unless_confirmed:` and/or `unless_heartbeat:`; `confirm()` disarms `unless_confirmed`, `commit()` disarms everything. A permanent plan with a backstop MUST reach `confirm()` or `commit()` on every non-refusing path (E0504) unless it declares `fires_by_construction: true`, in which case the verdict says the undo fires by construction.
- **Install before the first covered step; arm per `arm_before`.** The artifact and its marker directory are installed before the first covered step runs (E0406; in the Phase 0 model installation precedes the first covered step by construction, so the code is unreachable there and is an engine-time check unless installation gains a placement of its own). No step is considered committed before the backstop covering it is *armed*. Where `reach` is empty, arming may follow the step (late arming) and the verdict states the engine-only window. Where `reach` is non-empty, arming MUST precede the step (E0401).
- **Extension is an op.** `wane` renewal rearms the backstop *before* the new expiry is committed; a rearm that fails refuses the renewal (E0402 at check for an impossible ordering, R0404 at runtime). Renewal is accepted only within `renew_within` of expiry, anchored at renewal, never for an expired plan; the numbers are plan declarations.
- **Backstop locus viability is a precondition.** A `:target` backstop needs the scheduler binding to report presence on the host (E0403 at check, R0401 at apply) and the target bootstrapped (R0407).
- **Heartbeat.** `unless_heartbeat:` is sent by the engine at `interval` (default deadline/3; MUST be ≤ deadline/3, E0405) as a touch of a heartbeat file in the instance directory; the artifact compares that file's age to the deadline on the target's clock.
- **Time on the target.** At arm time the engine probes the target's clock; `|skew| > skew_tolerance` (site-declared, default 120s) refuses to arm (R0403). The verdict states scheduler granularity ("fires within about one minute after the deadline" for cron).

### 5.7 The interference query

Written as Datalog; implemented in Phase 0 as list comprehensions and in Phase 1 as iterator joins shaped as the Datalog, one function per relation with the names below (`ascent` sits behind the same signatures when scale demands it: the query is non-recursive, and result order, which the verdict's bytes depend on, is leaf order):

```
writes(S, F)      :- step(S), umbra(S, F).
maywrite(S, F)    :- step(S), penumbra(S, F).
needs(S, F)       :- step(S), undo_pre(S, F).
before(A, B)      :- lifo_order(A, B).
conflict(A, B, F) :- writes(A, F), writes(B, F), before(A, B), needs(A, F).
mayconflict(A,B,F):- maywrite(A, F), writes(B, F), before(A, B), needs(A, F).
mayconflict(A,B,F):- writes(A, F), maywrite(B, F), before(A, B), needs(A, F).
par_ok(P)         :- par(P), forall X,Y in children(P), X != Y => disjoint_umbra(X, Y).
```

`conflict` is E0301. `mayconflict` is E0302 under `:strict` and a verdict clause under `:warn`. Iterations of one `repeat over:` loop are disjoint by construction; conflicts between the loop body and steps outside it remain shape-level.

A fact is a shape **on a host**: `writes`, `maywrite` and `needs` range over (host, shape) pairs, resolved from each step's locus (`:target` is the owner host, `:controller` the controller, `host(...)` the named host), so the same shape on two hosts never conflicts (§5.12). A step whose host is bound at runtime is penumbral by host: every fact it touches is `maywrite`, and the verdict lists the binding under `unresolved_bindings`. `before` is undefined between children of one `par`; they are judged by `par_ok` alone and never as `conflict`. A conflict whose fact is an anchor declared twice on one shape is reported as E0305, the specific diagnosis, and not also as E0301.

### 5.8 The verdict

Schema at `docs/verdict-schema.json`, versioned; additions are allowed without a bump, removals and renames bump. Every "the verdict says" in this document names a field.

```json
{
  "verdict_version": 1,
  "plan": "breakglass", "host": "db-01",
  "status": "ok | refused",
  "intent": "temporary | permanent", "rehearsal": false, "mode": "manual | auto",
  "commit_step": null, "fires_by_construction": false,
  "reversible_through": 2, "holds_at": null,
  "point_of_no_return": { "step": 3, "guard": "fence_verdict(host)", "cost": "none", "ack": "thresh(1, humans())", "gate": "auth(:oncall)" },
  "reversible_back_to": { "from": 4, "to": 3 },
  "gate": { "satisfiable": true, "min_distinct_humans": 1, "zero_human_path": false, "window_s": 1800 },
  "backstop": { "triggers": [{ "after_s": 14400 }], "covers": [1, 2], "locus": "target",
                "installed_before": 1, "armed_after": 2, "late_arming_window": [1, 2],
                "scheduler": "cron", "granularity_s": 60, "self_enforced": true,
                "drift_policy": { "1": "defer", "2": "clobber" },
                "snapshots": { "location": "target", "cap_bytes": 1048576 } },
  "controller_only_undos": [4],
  "held_indefinitely": [], "induced_defer": [],
  "hosts_touched": { "1": [ { "host": "db-01", "directory": "target" } ],
                     "3": [ { "host": "db-01", "directory": "target" }, { "host": "bmc-01", "directory": "controller" } ],
                     "5": { "unresolved": "step 4 output target_host" } },
  "dispatch": { "source": "inventory", "host_contract_hash": "…" },
  "may_conflicts": [], "unresolved_bindings": [], "diagnostics": [],
  "steps": [
    { "n": 1, "op": "service_posture", "locus": "target", "undo": "restore 2 facts from snapshot", "undo_locus": "target",
      "refusal": "revert", "drift": "defer", "gate": null, "knell": null, "conditional": null, "footprint": ["…"] },
    { "n": 3, "op": "page_oncall", "locus": "controller", "undo": null, "undo_locus": "none", "refusal": "knell", "drift": null,
      "gate": { "expr": "auth(:oncall)", "step_digest": true, "window_s": null, "wait_alone_at_s": null },
      "knell": { "guard": "fence_verdict(host)", "cost": "none", "ack": "thresh(1, humans())" }, "conditional": null, "footprint": [] }
  ]
}
```

Field notes: `rehearsal` (a request dry-run, §7.9); `mode` (`manual` or `auto`: which hold and acknowledgement rules applied, and which hold clause the prose uses); `commit_step` (permanent plans); `held_indefinitely` (steps that hold without bound in a permanent plan); `induced_defer` (`:defer` steps under `mode: :auto`, §7.12); `steps[].conditional` (the foreign-region condition, §5.2; `backstop.conditional` is derived from it); `hosts_touched[].directory` (`target` or `controller`, §7.7); `gate.wait_alone_at_s` (earliest instant a gate is satisfiable by wait alone).

Prose rendering (Appendix A), one clause per field group, in this order: intent, reversibility, hold, point of no return, gate, step gates, backstop, conditionals, controller-only undos, hosts touched, dispatch, may-conflicts, unresolved bindings. Example:

```
breakglass on db-01: temporary; reverts at wane 4h; reversible through step 2; step 3 is a
point of no return, guard fence_verdict(host), cost none, acknowledged by thresh(1, humans());
step 3 gated by auth(:oncall); step 4 reversible back to step 3; gate satisfiable, minimum 1
distinct human; expiry backstop (after 4h) covers steps 1–2 on the target, installed before
step 1, armed after step 2, engine-only for steps 1–2 until armed, fires within ~1m after the
deadline, self-enforced on db-01, drift: 1 defer, 2 clobber; step 4 reverts only while the
engine lives; step 3 touches db-01, bmc-01; step 5 touches a host bound at runtime; clause
dispatch from inventory.
```

### 5.9 Runtime state

Five class rules, from which the diagram and the tier-4 truth table are derived:

1. **Terminal:** `Closed`, `Committed`.
2. **Bounded by `wane` in a temporary plan:** every non-terminal state except `DriftHeld` and `Stuck`; `Pending`'s bound is its approval window — a plan-entry gate with no `window:` on a site with no `max_wait` is E0506, since `Pending` would reserve umbras without bound. `wane` elapsing is always `Expired → Reverting`, never a hold, because the armed artifact fires on that deadline and the engine must agree with it. A temporary plan that can reach `Waiting`, `Held` or `Deferred` MUST declare `wane` (E0506).
3. **Unbounded, by declaration:** `DriftHeld` and `Stuck` in any plan (the alternatives are a policy override or a lie); `Held` and `Deferred` in a permanent plan (they wait for `resume`, `handoff-done`, `recant`, `commit` or `abandon`). A permanent plan's `Waiting` is bounded by the step's `window:` or the site's `max_wait` (E0506 if neither). Every unbounded state re-sends its notification on every reap pass, in any mode.
4. **Refusal during `Applying`** goes to `Reverting`, unless an earlier applied step has `refusal: :hold`, in which case to `Held{step}`. `:hold` under `mode: :auto` is not refused: in a temporary plan it is reverted at `wane`; in a permanent plan it holds until an operator acts. A `window:`/`max_wait` lapse that arrives before `wane` resolves per `on_lapse:` (`:revert` default; `:hold`; always `:revert` under `:auto`), journaled `WaitLapsed`.
5. **Commit** is reached from `Applying` by the item, from `Held` or `Deferred` by the verb. `commit`, `renew` and `confirm` on a plan whose intent does not admit them are R0102.

```
Unchecked ─check─▶ Checked ─request─▶ Pending{gate} ─approve─▶ Applying
Pending ─(approval window lapses)─▶ ApprovalExpired (reaped → Closed)      Pending ─cancel─▶ Closed
Pending ─host contract changed (R0301)─▶ Closed{refused}

Applying ─all steps done─▶ Applied{wane}                              (temporary)
Applying ─commit() item─▶ Committed                                   (permanent)
Applying ─refuse, no earlier :hold─▶ Reverting
Applying ─refuse, earlier :hold─▶ Held{step}
Applying ─gate|ack|unknown guard @step─▶ Waiting{step, reason} ─satisfied|acked|forced─▶ Applying
Applying ─defer @step─▶ Deferred{step, handoff} ─handoff-done (verb or probe)─▶ Applying

Applied ─recant─▶ Reverting          Applied ─suspend─▶ Suspended ─reestablish─▶ Applied
Applied ─renew (rearm first)─▶ Applied{wane'}                         (temporary only)
Applying | Applied ─confirm (item or verb)─▶ same state, backstop disarmed   (permanent only)
Held | Deferred ─commit (verb)─▶ Committed                             (permanent only)
Held ─resume─▶ Applying              Held | Waiting | Deferred ─recant─▶ Reverting
Waiting ─(window or max_wait lapses)─▶ Reverting | Held               (per on_lapse; WaitLapsed)

Reverting ─clean─▶ Closed            Reverting ─undo failed─▶ Stuck ─retry each pass─▶ Reverting
Reverting ─drift on a :defer step─▶ DriftHeld{steps}                  (engine or artifact)
DriftHeld ─recant --force=drift─▶ Reverting     DriftHeld ─recant─▶ DriftHeld (R0103)
Stuck | DriftHeld ─abandon (admin)─▶ Closed{abandoned}                (backstops disarmed where reachable, else left armed and journaled)

(any non-terminal state except DriftHeld, Stuck, Pending) ─(time ≥ wane)─▶ Expired ─▶ Reverting   (temporary plans)
DriftHeld | Stuck ─(time ≥ wane)─▶ unchanged; notify re-sent each pass
Held | Deferred in a permanent plan ─(any time)─▶ unchanged until an operator acts
```

What each state holds:

| State | Exclusivity class | Umbras in the cross-plan ledger | Instance directories |
|---|---|---|---|
| Unchecked, Checked | no | no | no |
| Pending, ApprovalExpired (until reaped) | yes | reserved | no |
| Applying, Applied, Waiting, Held, Deferred, Suspended, Reverting, Stuck, DriftHeld, Expired | yes | yes | yes (run-capable hosts only) |
| Closed, Committed | no | no | no (removed; orphans per §7.7) |

- `Applied` and `Suspended` exist only for temporary plans: a permanent plan goes from `Applying` to `Committed` and never rests, so `confirm` on a permanent plan happens while `Applying`. `renew` is meaningful while `Applying` as well as `Applied`, since `wane` is anchored at approval.
- All observations take `now`. `Expired` and `ApprovalExpired` are observed, never scheduled. The boundary is closed: observed *at* the instant is expired.
- `Applying` is persisted (write-ahead) before any `do`. A crash in `Applying` demotes to `Reverting` at boot.
- Apply is atomic-or-reported: a failed step is itself reverted (it may be half-applied). `Stuck` is persisted and retried every pass; `Closed{reverted}` is journaled only when clean.
- One instance per (host, exclusivity class): a second is refused with exit 75 (R0101). Every host a step touches acquires the class for the instance's life.

### 5.10 Journal model

```
Entry = { seq, prev_hash, hash, at, plan, instance, host, event, secret_labels: [Label], sig: Option<Sig> }
event ∈ { Checked, Requested, ProofAccepted{scope, authenticator, submitter}, Approved{rehearsal}, ApprovalExpired, Cancelled,
          Applying{step, undo_line}, StepDone{step}, StepFailed{step, error}, Applied, Renewed, Confirmed,
          Committed{by, reason}, Recant, Reverting{steps}, Reverted, Stuck{steps}, Expired, Closed{reason},
          BackstopFired{step}, BackstopFiredAfterAbandon{host, steps}, Held{step}, Resumed{step, by},
          Deferred{step, handoff}, HandoffDone{step, by}, Suspended, Reestablished,
          Waiting{step, reason}, WaitLapsed{step, reason}, StepGateRequested{step, step_digest}, StepGateSatisfied{step},
          AckRequested{step, cost}, KnellAcknowledged{step, cost, by}, Denied{gate, reason},
          FootprintViolation{step, facts}, DriftClobbered{step, facts}, DriftHeld{step, facts},
          HostContractChanged{expected, observed}, Refused{reason},
          SecretRevealed{label, acceptor}, SecretUndelivered{label}, SecretDropped{label, reason},
          StagedRemoved{step, reason}, InstanceDirOrphaned{host, instance, armed}, Reclaimed{host, instance, forced, reason},
          Abandoned{steps_not_reverted, artifacts_left_armed, by, reason},
          HookRegistered{name, registrar, connection}, HookDeregistered{name, registrar, reason},
          OperatorConnected{identity, admin}, OperatorDisconnected{identity}, KeyRotated{old_pub, new_pub}, Migrated{from, to, by} }
```

- `hash = H(prev_hash || canonical(entry without hash, sig))`; genesis `prev_hash = 0`. Canonicalisation is length-prefixed, field-ordered, domain-separated (`rue-journal`).
- Secrets and tokens are never entries and never fields. `secret_labels` lists the *labels* of secrets an event delivered, so a reader knows a reveal happened without any value having been written.
- **Signing.** `journal … sign: key(path)` names an Ed25519 private key (OpenSSH format) held by the engine; `sig` is an SSHSIG over the canonical entry, namespace `rue-journal`; `rue journal verify` checks the chain and, given `--key`, every signature (`ssh-key` crate, in-process). Key rotation is a `KeyRotated` entry signed by the old key.
- Every plan entry that used a hook references the hook's `HookRegistered` entry by `seq`. A refused approval (`Denied`) is journaled; other refusals are not (nothing happened). This is the one deliberate exception.

### 5.11 Gates, acknowledgements, and proofs

A gate is a policy over authenticators, groups and waits in the weighted-threshold shape (Antelope/Miniscript lineage), so `check` can compute satisfiability, the minimum number of distinct human authenticators on any satisfying path, and the worst-case wait before a plan can be requested.

```
gateexpr := "thresh(" INT "," factor ("," factor)* ")" | factor
factor   := "auth(" ATOM ("," "weight:" INT)? ")"        -- an authenticator id published by the approval binding
          | "humans(" ("weight:" INT)? ")"                -- any authenticator the binding marks human
          | "group(" gateexpr ("," "weight:" INT)? ")"
          | "wait(" DURATION ("," "weight:" INT)? ")"
```

- **Rue owns the shape; the binding owns trust.** The approval binding publishes the authenticator ids it can verify, each flagged `human: true|false`. `check` refuses a gate naming an unknown id, an unsatisfiable threshold, or one counting the requester (E0508). A gate satisfiable with no human refuses (E0509) unless the plan declares `allow_zero_human: true`; under `mode: :auto` it always refuses. The verdict reports satisfiability, minimum distinct humans, and the earliest instant each gate is satisfiable by wait alone.
- **Rue supplies a request digest, not a challenge.** `request_digest = H(canonical(nonce, plan_id, instance, owner_host, params_hash, host_contract_hash, wane, requested_at, gate_hash, plan_content_hash))`, domain-separated `rue-request`, with a 32-byte nonce from the engine's CSPRNG (core stays free of randomness). A step gate uses `step_digest = H(request_digest, step_index)`; an ack uses the same with scope `ack`. The binding renders whatever human-facing challenge it likes over the digest and verifies proofs against it; rue never sees proof bytes, only `verified: true` for an authenticator id, and journals the submitting operator beside it.
- **Binding contract.** A proof MUST be bound to the digest and its scope (`plan`, `step`, or `ack`), so a proof for one request or step verifies for no other, and a plan, parameter or host-contract change after request invalidates every accumulated proof (the engine re-derives the host contract at request, approval and apply; a change is R0301). Replay resistance is a contract rue requires and the conformance suite tests, not a mechanism rue implements.
- **Plan entry.** Proofs accumulate across `rue approve` calls; `wait` weight accrues from `requested_at`; the reap pass opens the plan the instant the threshold is crossed; the approval window (`gate …, window:`) lapses fail-closed; `wane` is anchored at approval, never at request. The requester's own authenticators never count toward a plan-entry or step `gate:`. The requester is therefore an input to `check` (`rue check --as <authenticator>`, or the connected operator's identity), so E0508 is decidable offline.
- **Step gates.** `gate:` on a step enters `Waiting{gate}`; `wait` accrues from `StepGateRequested`; `rue approve --step N` submits a proof in the `step` scope.
- **Acknowledgements.** A knell's `ack:` is a gate in the `ack` scope (default `thresh(1, humans())`, or `:none` with a reason for a knell acknowledged by its own guard, e.g. a fence driver's verified-off). Accepting a cost is the operator's own recorded decision, so the requester exclusion does not apply to acks. `rue ack --step N --reason < token` is `rue approve --step N` in the `ack` scope plus a journaled reason. Under `mode: :auto` every knell MUST declare `ack: :none` and every step gate MUST be satisfiable without a human (E0507). A knell with both `gate:` and `ack:` is legal: distinct scopes, distinct waits; the checker warns when both name the same single authenticator.

### 5.12 Multi-host plans and cross-plan interference

- A **plan instance** is keyed by `(plan_id, params_hash, owner_host)`; the owner host is the `--host` argument. Ops with `locus: host(expr)` may act on other hosts; every host touched acquires its exclusivity class for the instance's life. A multi-host plan whose owner cannot be inferred is E0409. The verdict is per owner host; `explain` lists every touched host per step. A step is **deferred** when its host is not the owner and no site transport reaches it, or when its host is bound at runtime; the verdict lists it under `deferred` and `explain` prints its `handoff_done`.
- **Cross-plan interference is checked at request and reserved from `Pending`.** The engine keeps, per host, the union of active and pending instances' umbras; a new instance overlapping one is refused at request (R0203) *regardless of exclusivity class*, before any proof is collected. Reservation is released on cancel, lapse or close. A request dry-run reserves nothing.

### 5.13 Secrets

- An op output marked `secret` flows through `|>` as `Secret<V>`, never appears in `explain`, any journal entry, argv, a shell history, an artifact, or a hook message other than the four permitted (§7.5), and is delivered exactly once.
- **Delivery** happens when the producing step's completion is journaled — the credential is live from that instant and the step's undo is what revokes it — to the first acceptor in `secrets deliver_to:` that returns `accepted: true`; the engine then drops its copy. Built-ins: `requester()` (the attached client, if any) and `hold(until: :wane | DURATION)` (engine memory only, never the store; fetched once by `rue reveal`; dropped at the bound; `until: :wane` on a permanent plan resolves to the site's `max_wait` or is refused, R0104). A secret still held when the instance reverts, expires or is abandoned is dropped and journaled `SecretDropped{reason}`; a daemon restart drops every held secret and journals `SecretDropped{reason: :daemon_restart}` at boot. A plan with a `secret` output and no declared `deliver_to` refuses at check (E0606); a list every acceptor declines yields `applied; secret undelivered` and exit 7.
- **Placement.** A `run` primitive takes secrets only via `env:`/`stdin:`, both implemented by the executor as a stdin preamble (§7.4), or via `stage()` (a mode-0600 file in the instance directory, auto-added to the step's `Owned` footprint, removed after the step, by the artifact when it fires, and by boot recovery for any instance not `Applying`). A `Secret` interpolated into a `run` string is E0209; referenced from a `:target` undo body, E0210; on an executor that cannot honour the preamble, E0211 at check when known statically.
- A plan routing a secret to a non-secret sink is E0411.

---
## 6. The surface (rue-surface)

### 6.1 Feel and totality

Elixir-flavoured, total, first-order. `do … end` blocks, keyword-list parameters, `|>` as `seq`, pattern-matched clauses on host shape, `import` across files. No type syntax. No `fn`, no `case` over arbitrary values, no `Enum`, no recursion; `repeat` is bounded by a literal or a set-valued fact with a literal cap. The first time someone writes a closure or a recursive call, the diagnostic (E0106) teaches the bounded form.

### 6.2 Lexical rules

A `.rue` file is UTF-8, `\n` line endings, and begins with a version marker.

```
file      := "rue" INT NEWLINE top*                     -- the language version this file targets (E0105 if missing or newer)
top       := site | import | defprobe | defprim | defop | defplan | defrole | defprotocol | defimpl
comment   := "#" .* NEWLINE                             -- preserved by the lossless tree and `fmt`
```

Tokens: `NAME` (`[a-z_][a-z0-9_]*` with an optional trailing `?`, as the builtins `defined?` and `member?` spell it), `UPPER_NAME` (`[A-Z][A-Za-z0-9_]*`, an environment variable's name as a record key), `ATOM` (`:` NAME, or `:"..."` for an atom with characters a NAME cannot carry, such as `:"corpse:node-a"`), `INT`, `FLOAT`, `BOOL`, `STRING` (double-quoted, `#{expr}` interpolation with braces balanced inside it), `DURATION` (`INT ("ms"|"s"|"m"|"h"|"d")` — bare tokens, never strings), keywords (§6.6), punctuation `( ) [ ] { } , : | |> = == != < <= > >= + - * / % . do end else`. Keywords are contextual: a keyword is a NAME the parser reads by position, so `user`, `content` and `window` serve as keyword-argument names too. A comment is trivia anywhere a line ends or begins: trailing a statement, alone inside a body, alone at the top; the lossless tree keeps it and `fmt` writes it back. A newline ends a statement everywhere; brackets close on the line that opened them.

### 6.3 Statement grammar

```
site      := "site" "do" sitedecl* "end"
sitedecl  := ("inventory" "from:" | "journal" "to:" | "approval" "via:" | "secrets" "from:"
            | "secrets" "deliver_to:" | "notify" "via:" | "execute" "via:" | "backstop" "scheduler:")
              bindexpr ("," kw)* NEWLINE                -- a binding call may carry keywords of its own: hook(:bmc_api, transport: :api)
           | "max_wait" DURATION NEWLINE
           | "skew_tolerance" DURATION NEWLINE
           | "operators" "do" ("identity" ATOM "," "user:" (STRING | ":socket_owner")
                 ("," "operator_for:" (atomlist | ":all"))? ("," "admin:" BOOL)? ("," "subscribe:" atomlist)? NEWLINE)* "end"
           | "hooks" "do" ("registrar" ATOM "," "user:" (STRING | ":socket_owner") "," "may_register:" atomlist NEWLINE)* "end"
atomlist  := "[" (ATOM ("," ATOM)*)? "]"                -- every list in the surface is comma-separated
bindexpr  := call | "[" call ("," call)* "]"            -- file("…"), hook(:audit), [local(), ssh()], [requester(), hold(until: :wane), hook(:escrow)]

import    := "import" STRING ("as" NAME)? NEWLINE

defprobe  := "defprobe" ATOM params? ("," kw)* "do" probebody "end"
probebody := ("run" STRING | "hook" ATOM) NEWLINE ("locus" ATOM NEWLINE)? ("equivalence" ATOM NEWLINE)?
             ("produces" factshape ("," factshape)* NEWLINE)? ("static" BOOL NEWLINE)?

defprim   := "defprim" ATOM params? "do" "run" STRING ("," "classes:" record)? NEWLINE "end"   -- a run template with declared argument classes

defop     := "defop" ATOM "," pattern params "do" opbody "end"
params    := ("," NAME ":" (NAME | expr))*             -- parameter declarations: `ack: ack` a required parameter, `drift: :defer` one with a default;
                                                       -- a declared parameter may stand wherever the body grammar names a literal (`drift: drift`, `ack: ack`),
                                                       -- and an `ack` parameter bound to `:none` at the call takes a sibling `reason:` there, as `refusal:` does
opbody    := ("footprint" (fpentry ("," fpentry)*)? NEWLINE)   -- empty for a knell that touches nothing
             ("reach" transport ("," transport)* NEWLINE)?
             ("pre" guard ("," guard)* NEWLINE)?
             ("do:" body NEWLINE)
             ("undo:" undo NEWLINE)?
             ("undo_pre" guard ("," guard)* NEWLINE)?
             ("post" guard ("," guard)* NEWLINE)?
             ("undo_locus:" ATOM NEWLINE)?                 -- :target | :controller | :none
             ("refusal:" refusal NEWLINE)?                 -- default :revert
             ("drift:" ATOM NEWLINE)?                      -- :clobber | :defer; default by kind
             ("outputs" output ("," output)* NEWLINE)?
             ("exclusivity:" ATOM NEWLINE)?
             ("locus:" locus NEWLINE)?
             ("handoff_done:" call NEWLINE)?
             ("suspend:" body NEWLINE "reestablish:" body NEWLINE)?
fpentry   := kind ":" factshape                           -- owned: file("/etc/x"), region: file("/etc/x", anchor: "rue"), …
kind      := "owned" | "region" | "modified" | "derived" | "append_only" | "held"
factshape := call | NAME ("." NAME)*
refusal   := ":revert" | ":hold" | ":hold" "," "via:" ATOM
           | "knell" ("," "guard:" guard)? "," "cost:" (call | ":none" "," "reason:" STRING)
                     ("," "ack:" (gateexpr | ":none" "," "reason:" STRING))?
undo      := ":restore" | body | "compensate:" body
output    := NAME ("," "secret:" BOOL)?
locus     := ":controller" | ":target" | "host(" expr ")"

defplan   := "defplan" ATOM "," pattern params "do" planopts item* "end"
planopts  := -- an unordered set of the following lines, each at most once
             ("gate" gateexpr ("," "window:" DURATION)? ("," "allow_zero_human:" BOOL)? NEWLINE)?
             ("wane" DURATION ("," "renew_within:" DURATION)? NEWLINE)?
             ("backstop" "trigger:" trigger ("," "locus:" ATOM)? ("," "arm_before:" (ATOM | INT))? NEWLINE)?
             ("fires_by_construction:" BOOL NEWLINE)?
             ("strictness:" ATOM NEWLINE)?                 -- :strict (default) | :warn
             ("mode:" ATOM NEWLINE)?                       -- :manual (default) | :auto
             ("exclusivity:" ATOM NEWLINE)?
             ("require" "journal:" ATOM NEWLINE)?         -- :chained | :signed
trigger   := "[" triggerpart ("," triggerpart)* "]"
triggerpart := "after:" DURATION | "unless_confirmed:" DURATION | "unless_heartbeat:" DURATION ("," "interval:" DURATION)?

item      := step | pipeline | par | slot | knellitem | "confirm()" NEWLINE | "commit()" NEWLINE
           | preflight | observe | assert | repeat | whenblock
step      := call ("," "gate:" gateexpr)? ("," "window:" DURATION)? ("," "on_lapse:" ATOM)?
                  ("," "force:" "[" forcename ("," forcename)* "]")? ("as" NAME)? NEWLINE
forcename := ATOM | "drift" | "unknown"
pipeline  := step ("|>" step)+
par       := "par" "do" item* "end"
slot      := "slot" ATOM NEWLINE
knellitem := "knell" step
preflight := "preflight" "do" guard* "end"
observe   := "observe" call "as" NAME NEWLINE
assert    := "assert" guard ("," "window:" DURATION)? ("," "on_lapse:" ATOM)? NEWLINE
repeat    := "repeat" INT "as" NAME "do" item* "end"
           | "repeat" "over:" expr ","? "as" NAME "," "max:" INT "do" item* "end"   -- `fmt` writes the comma
whenblock := "when" guard ("," "window:" DURATION)? ("," "on_lapse:" ATOM)? "do" item* ("else" item*)? "end"

defrole   := "defrole" ATOM "do" contribution* "end"
contribution := ATOM (INT)? item                          -- slot name, priority (default 100, lower first), an item
defprotocol := "defprotocol" ATOM ("," "inverse:" ATOM)? "do" ("default" item)? "end"
defimpl   := "defimpl" ATOM "," "for:" ATOM "do" item* "end"

pattern   := NAME | "%{" (NAME ":" pat) ("," NAME ":" pat)* "}" ("=" NAME)?
pat       := ATOM | STRING | INT | NAME | "[" (pat ("," pat)*)? "]" | "_"   -- a list pattern matches any of its members
guard     := expr | "force:" "never" "," expr
kw        := NAME ":" expr
```

`window:`/`on_lapse:` on a step govern whichever wait the step produces (gate, ack, guard). `gateexpr` is §5.11.

### 6.4 Bodies and reuse

A body is a list of **primitives**; the checker sees primitives, never shell. Every primitive declares which arguments are target-local and which may carry controller values (the input to closure analysis, E0202).

```
body := "[" prim ("," prim)* "]" | prim
prim := "run(" STRING ("," "env:" record)? ("," "stdin:" expr)? ("," "idempotent:" BOOL)? ")"
      | "write(" factshape "," "content:" expr ")" | "remove(" factshape ")" | "append(" factshape "," "line:" expr ")"
      | "region_set(" factshape "," "content:" expr ")" | "region_clear(" factshape ")"
      | "stage(" NAME "," "content:" expr "," "mode:" INT ")"
      | "hook(" ATOM ("," kw)* ")" | "install(" ATOM ")" | "release(" ATOM ")"   -- hook admits idempotent: BOOL among its keywords
      | call                                     -- a tenant-declared defprim
```

An undo is provably idempotent (E0208 otherwise) when it is `:restore` or `compensate:`, or a body whose every primitive is: the fact primitives, `install` and `release` are by construction; a `run` or a `hook` is only when it carries `idempotent: true`, the author's declaration that running it twice ends where once does.

`run` strings are the only place shell text exists. Interpolation into a `run` string is quoted by the renderer for the step's OS family (POSIX single-quote with `'\''` escaping; PowerShell single-quote doubling), and the whole string is then embedded per the artifact's language (a Python literal with backslash escapes when the artifact is Python); a value that cannot be safely quoted is E0109 (a NUL anywhere; a control character other than tab, newline and return in a shell family).

Reuse mechanisms and their totality rules:

| Mechanism | Rule |
|---|---|
| Pipelines | The value flowing through `\|>` is the plan-so-far plus named outputs; never a function. `a \|> b` desugars to `seq(a, b)` |
| Modules | `defop`/`defplan` are named, parameterised templates expanded at check time. `import` is file inclusion with namespacing. No first-class op values as arguments |
| Clauses | Dispatch on host-contract facts frozen before the plan runs, resolved at check time per inventory host. Patterns tried in file order; first match wins; no match is E0112; a pattern on a non-static fact is E0111; identical patterns are E0103 |
| Roles and slots | `defrole` contributes items into named `slot`s with a priority; multiple roles on one host contribute all; the checker proves disjointness or refuses |
| Protocols | `defprotocol`/`defimpl` name a capability with a per-role body and a default; a protocol may declare a paired inverse and every `defimpl` must supply both |

### 6.5 Expressions and kinds

```
expr   := or
or     := and ("or" and)*          and := not ("and" not)*        not := "not" not | cmp
cmp    := add (("<"|"<="|">"|">="|"=="|"!=") add)?
add    := mul (("+"|"-") mul)*     mul := unary (("*"|"/"|"%") unary)*     unary := "-" unary | atom
atom   := INT | FLOAT | STRING | ATOM | DURATION | BOOL | ref | call | "(" expr ")" | record | list
ref    := NAME ("." NAME)*          -- a fact path, a plan param, host.<field>, or <alias>.<output>
call   := (NAME ".")* NAME "(" (expr ("," expr)* ("," kw)*)? ")"   -- qualified: an imported name (t3.pf_allow(...)) or a dotted fact shape (bmc.account("bg"))
record := "%{" (key ":" expr) ("," key ":" expr)* "}"         list := "[" (expr ("," expr)*)? "]"
key    := NAME | UPPER_NAME | STRING                          -- %{"hvac-1": :off}, %{BMC_PW: secret(:pw)}
STRING := '"' (char | "#{" expr "}")* '"'                    -- Secret if any part is Secret
```

Builtins: `if/3 defined?/1 unknown?/1 all_eq?/2 any_eq?/2 count_eq/2 min/2 max/2 abs/1 to_s/1 to_s/2 len/1 member?/2 secret/1`. No user-defined functions.

Types are inferred, never written: every builtin, primitive and op parameter has a kind (`int`, `float`, `str`, `atom`, `duration`, `bool`, `list`, `record`, `fact`); a value of one kind flowing into a position of another is E0107 ("port expects int, got str from step 2's output"). A tenant may pin a parameter's kind (`port: :int`) when inference would be ambiguous.

### 6.6 Keywords (reserved)

`rue site inventory journal approval notify execute secrets backstop scheduler from to via deliver_to max_wait skew_tolerance operators identity user operator_for admin subscribe hooks registrar may_register import as defprobe defprim defop defplan defrole defprotocol defimpl footprint reach pre do undo undo_pre post undo_locus refusal drift outputs exclusivity locus handoff_done suspend reestablish trigger arm_before wane renew_within gate allow_zero_human strictness mode require fires_by_construction par slot knell confirm commit preflight observe assert when else repeat over max force never hold via cost ack reason secret owned region modified derived append_only held after unless_confirmed unless_heartbeat interval window on_lapse unknown guard compensate run env stdin write remove append region_set region_clear stage content line hook install release default inverse for static equivalence produces classes`

### 6.7 Diagnostic codes

Golden-tested text with `file:line:col`, expected/found, nearest-name suggestion. Codes grow, never renumber (renumbered once, before Phase 0, to close the table's gaps; frozen from Phase 1).

| Code | Meaning |
|---|---|
| E0101 | Parse error (expected/found) |
| E0102 | Unknown name (with nearest-name suggestion) |
| E0103 | Duplicate definition, or indistinguishable clauses |
| E0104 | Import cycle |
| E0105 | Language version marker missing, or newer than this compiler |
| E0106 | Non-total construct (closure, recursion, unbounded repeat) |
| E0107 | Kind mismatch |
| E0108 | Comparison against `:unknown` |
| E0109 | Interpolated value cannot be safely quoted for the target OS family |
| E0110 | Output referenced before its step, or across a `par` sibling |
| E0111 | Clause pattern names a fact not in the host contract |
| E0112 | No clause matches this host |
| E0113 | `repeat over:` list is not set-valued |
| E0114 | `when` arms bind an output under different kinds |
| E0201 | Op without undo is not `knell`, or vice versa |
| E0202 | `:target` undo is not closed over target-local commands and facts |
| E0203 | `undo_locus: :none` with an undo body |
| E0204 | `knell` without a cost probe or `cost: :none` reason |
| E0205 | `held` footprint without `suspend`/`reestablish` |
| E0206 | Secret-producing op reachable from `reestablish` |
| E0207 | Computed or compensating undo without `undo_pre` |
| E0208 | Undo not provably idempotent |
| E0209 | `Secret` interpolated into a `run` string |
| E0210 | `Secret` referenced from a `:target` undo body |
| E0211 | Secret `env:`/`stdin:` on an executor that cannot honour the stdin preamble |
| E0301 | Footprint conflict (umbra) |
| E0302 | May-conflict (penumbra), strict mode |
| E0303 | `par` children not umbra-disjoint |
| E0304 | `reach` op inside `par` |
| E0305 | Same anchor declared twice on one fact within a plan |
| E0401 | `reach` op without a preceding armed `:target` backstop |
| E0402 | Renewal would commit before backstop rearm |
| E0403 | Backstop locus not viable on host, or no artifact template for its OS |
| E0404 | `mode: :auto` plan contains `force:` |
| E0405 | Heartbeat interval not ≤ deadline/3 |
| E0406 | Artifact would be installed after a covered step |
| E0407 | `:target` undo locus on a host with no run-capable executor |
| E0408 | Snapshot for a `:target` undo exceeds the declared cap |
| E0409 | Multi-host plan without an inferable owner host |
| E0410 | `reach` op whose undo is `drift: :defer` |
| E0411 | Secret routed to a non-secret sink |
| E0501 | Intent undeterminable: both `wane` and `commit()`, or neither |
| E0502 | `commit()` is not the last item on its path |
| E0503 | Temporary plan's backstop `after:` differs from its `wane` |
| E0504 | Permanent plan with a backstop has a path reaching neither `confirm()` nor `commit()` |
| E0505 | Permanent plan has a non-refusing path that never reaches `commit()` |
| E0506 | Unbounded wait: temporary plan without `wane` can reach `Waiting`/`Held`/`Deferred`, or a permanent plan's wait has neither `window:` nor a site `max_wait` |
| E0507 | `mode: :auto` plan has a step gate needing a human, or a knell whose `ack:` is not `:none` |
| E0508 | Gate unsatisfiable, names an unknown authenticator, or counts the requester |
| E0509 | Gate satisfiable with zero human authenticators and no `allow_zero_human` |
| E0601 | Unresolved binding |
| E0602 | Binding contract violation |
| E0603 | No journal declared; refusing to apply |
| E0604 | No `operators` block; refusing to start outside daemon dry-run mode |
| E0605 | A `hook()` binding is declared but no `hooks` registrar block names who may register it |
| E0606 | Plan has a `secret` output and the site declares no `secrets deliver_to` |
| E0607 | `inventory from: hook()` is checked with no record to check against; name one with `rue check --inventory` |
| E0608 | An action the host's executor cannot perform: a `hook(...)` action, or a probe with no `run` body, on a host reached by `local()` or `ssh()` |

### 6.8 CLI and exit codes

```
rue check   <plan.rue> [--host H] [--json]           parse, resolve, check; print verdict
rue explain <plan.rue> [--host H]                    per-host expanded steps with undo lines, loci, policies, gates
rue render  <plan.rue> [--host H]                    expected end-state facts; touches nothing
rue artifact <plan> --host H --instance ID [--set k=v]... print the backstop artifact a :target backstop installs on H (Phase 1: over the plan IR)
rue apply   <plan.rue> --host H [--set k=v]... [--mode auto] [--ack N:"reason"]... [--dry-run]
rue reveal  <instance>                               fetch a secret held by hold(), exactly once
rue approve <instance> [--step N] < token            submit a proof for a plan-entry or step gate
rue ack     <instance> --step N --reason "…" < token approve in the ack scope plus a journaled reason
rue recant  <instance> [--force=<name,…>]            run the undo; names are guards or the classes drift, unknown
rue renew   <instance> --wane 2h                     extend a temporary plan; rearms the backstop first
rue confirm <instance>                               disarm an unless_confirmed backstop
rue commit  <instance> --reason "…"                  end a permanent plan from Held or Deferred
rue resume  <instance>                               continue a Held instance from its held step
rue handoff-done <instance> --step N                 continue a Deferred instance after the handoff
rue abandon <instance> --reason "…"                  admin: close a Stuck or DriftHeld instance, world left as-is
rue status  [instance]                               observed state, remaining wane, stuck and held sets
rue journal verify <file> [--key K]                  verify the chain (and signatures) end to end
rue reclaim <host> <instance> [--force --reason "…"] remove an orphaned instance directory
rue bootstrap <host>                                 verify a target's rue_root; print the admin commands if missing
rue doctor  [--canary]                               bindings, executors, schedulers, bootstrap, sinks, modes
rue fmt     <file>                                   lossless formatter
rued migrate [--dry-run]                             migrate the instance store schema, explicitly
```

Admin verbs (`abandon`, `reclaim`, `bootstrap`, `doctor --canary`, `journal verify` against the live store) require an operator declared `admin: true` (§7.4); `rued migrate` runs with the daemon stopped and is protected by OS permissions on the store.

Exit codes: `0` applied / ok; `1` refused, check failed, diagnostics; `2` usage, contract, identity or scope error; `3` **held**; `4` **stuck**; `5` **deferred**; `6` **waiting** (pending plan-entry approval, or a mid-plan gate, ack or unknown guard — the verdict line names the step); `7` applied but a **secret was undelivered**; `8` **drift-held**; `75` exclusivity held by another instance. Codes 3–8 are never `0`. stdout is data; stderr is diagnostics; every verb that acts on the world ends in a verdict line, and it is the last line printed.

### 6.9 Front-end stack

Lexer `logos`; a hand-written recursive-descent parser with statement-level recovery emitting `rowan`'s lossless green tree directly (error recovery is required; parse errors are E0101 with expected and found, one per line at most so a recovered line cannot cascade, unknown names E0102 with the nearest-name suggestion, import cycles E0104; `chumsky`, named here before Phase 2, is still an alpha and would need a second pass to reach a lossless tree); tree `rowan` (lossless, so `explain`, `fmt` and editor tooling round-trip source); diagnostics `miette`; resolver producing one `core::Plan` per host. A file's site is its own `site do` when present, else the site of exactly one imported file (two imported sites and none local is E0103), and a path in a site resolves relative to the file that declares it. `rue fmt` is idempotent and byte-preserves every tenant file; it refuses a file with parse errors rather than rewrite it.

---
## 7. The engine (rue-engine, rued)

### 7.1 Responsibilities

Bindings resolution and contract checks at load; the write-ahead lifecycle (§5.9) over a locked, atomic, versioned instance store; executor dispatch by locus; footprint snapshot and post-`do` drift diff; instance directories, backstop rendering, installation, arming, rearming and disarming through the scheduler binding; heartbeat; journal chaining and synchronous multi-sink delivery; the reap pass (observe expiry and lapses, retry stuck, lapse pending, read fired markers, re-send notifications for unbounded states); boot recovery (demote `Applying`, reestablish `Held` resources, reconcile instance directories, settle); dry-run; the control channel with its identity model; hook registration; the cross-plan ledger.

### 7.2 Executor trait (object-safe)

```rust
pub trait Executor: Send {
    fn locus(&self) -> LocusKind;                                  // Local | Ssh | Hook(name) | ...
    fn capabilities(&self) -> ExecCaps;                            // { filesystem: bool, stdin_preamble: bool }
    fn run(&mut self, host: &HostRecord, body: &Body, env: &Env) -> Result<Output, ExecError>;
    fn run_with_preamble(&mut self, host: &HostRecord, instance: &InstanceId, body: &Body, secrets: &[(Name, Secret)]) -> Result<Output, ExecError>;
    fn observe(&mut self, host: &HostRecord, probe: &Probe) -> Result<Fact, ExecError>;
    fn bootstrap_state(&mut self, host: &HostRecord) -> Result<BootstrapState, ExecError>;   // rue_root, group, instances/, lock, modes
    // Instance directory (only when capabilities.filesystem):
    fn instance_dir_create(&mut self, host: &HostRecord, instance: &InstanceId) -> Result<(), ExecError>;
    fn instance_dir_remove(&mut self, host: &HostRecord, instance: &InstanceId) -> Result<(), ExecError>;
    fn instance_dir_list(&mut self, host: &HostRecord) -> Result<Vec<InstanceDirState>, ExecError>;
    fn put_file(&mut self, host: &HostRecord, instance: &InstanceId, rel: &str, bytes: &[u8], mode: u32) -> Result<(), ExecError>;
    fn replace_file(&mut self, host: &HostRecord, instance: &InstanceId, rel: &str, bytes: &[u8]) -> Result<(), ExecError>;   // write-temp-then-rename
    fn get_file(&mut self, host: &HostRecord, instance: &InstanceId, rel: &str) -> Result<Vec<u8>, ExecError>;
    fn host_lock(&mut self, host: &HostRecord) -> Result<Box<dyn HostLockGuard>, ExecError>;   // released on drop
}
```

Executors run with the store lock released. An executor that returns empty output where output was promised is `ExecError::Silent`, treated as a refusal.

### 7.3 Bindings and their contracts

Every external is a declared binding in the `site` block; the engine ships at most one generic built-in per kind. A plan with an unresolved binding refuses to run (E0601); a contract violation is E0602 at load and R0303 at runtime.

| Binding | Contract | Generic built-ins |
|---|---|---|
| `inventory` | Produces `HostRecord { name, address, os, roles: [Atom], reach: [Transport], facts: Map }` (Appendix C); names unique; `reach` non-empty | `rue_toml(path)`, `hook(name)` |
| `journal` | Accepts a chained (and possibly signed) `Entry`; acks synchronously; refusal ⇒ engine refuses | `file(path)`, `stdout()`, `hook(name)` |
| `approval` | Publishes authenticator ids with `human` flags; renders a challenge over rue's digest and scope; verifies one proof for one authenticator over that digest (`verified: true\|false`). Threshold evaluation is rue's | `always()` (daemon dry-run only), `hook(name)` |
| `secrets from` | Resolves `{:secret, id}` references at load; refuses missing; checks file mode | `file(path)`, `hook(name)` |
| `secrets deliver_to` | Ordered acceptors for a `Secret`; first `accepted: true` takes it. Required when any plan has a `secret` output (E0606) | `requester()`, `hold(until:)`, `hook(name)` |
| `notify` | Delivers `(level, subject, body)`; bounded; never fatal to a plan | `stdout()`, `hook(name)` |
| `execute` | One or more `Executor`s | `local()`, `ssh()`, `hook(name)` |
| `backstop scheduler` | Installs, arms, rearms, disarms and reports presence of the target-side scheduler entry for a rendered artifact | `cron()`, `task_scheduler()`, `hook(name)` |

Site-level policy numbers — `max_wait`, `skew_tolerance`, renewal windows, approval windows — are declarations with no engine defaults except `skew_tolerance` (120s). A site with no journal refuses to apply (E0603).

### 7.4 The control channel and identity

One socket (mode `0660`, group `rue`; on Windows a named pipe with a DACL for group `rue`), one handshake, then roles. `docs/control-protocol.md`, versioned, specifies: the `hello` frame, request/response framing with ids, every CLI verb as a message, subscription and notification frames, error frames carrying R-codes, and the version-refusal frame (R0501).

- **Identity.** A connection begins with `{"hello": {"proto": N, "identity": ":name"}}`; the client's OS identity is read from peer credentials (`SO_PEERCRED`/`LOCAL_PEERCRED`; the pipe client's SID on Windows) and must map to a **declared operator** in the site block. There are no implicit operators: the socket owner and any member of group `rue` are refused without a declaration (R0503). `identity :owner, user: :socket_owner` is the spelling for the account `rued` runs as; `operator_for: :all` is the spelling for every plan; `admin: true` grants the non-plan verbs and implies nothing about plan scope. Outside daemon dry-run mode a site with no `operators` block refuses to start (E0604).
- **Scope.** An act outside `operator_for` is R0504; an admin verb by a non-admin is R0506. Every admin act carries a `--reason` journaled under the peer-credential identity, and the notify binding is sent. The CLI's identity is whatever operator its OS user maps to, journaled as the *submitter* of every proof beside the proof's own authenticator.
- **Embedded hosts.** A host process that wants to approve, force, resume, recant or receive instance notifications connects as a control client with its own declared identity, scoped by `operator_for` and `subscribe`; it runs its plans in `mode: :manual` with itself as operator. `mode: :auto` is reserved for unattended triggers with no principal present.
- **Hook registration** happens on the same socket after `hello`: `{"register": {"name", "kinds", "protocol"}}`, accepted only from a declared **registrar** for a hook name in its `may_register` (R0505); a hook is trusted by name for everything it serves, so the registrar declaration is the site's statement of that trust. A child the engine spawned over stdio has no peer credentials and is `:socket_owner` by construction — it must still be a declared registrar. A `hook()` binding with no registrar block is E0605. Registrations and operator connections are journaled (§5.10).
- **The stdin preamble.** `env:` and `stdin:` on `run` are implemented by the executor as NUL-separated `key=value` pairs read into the environment by a rue shim on the target (in the instance directory) before exec; `ssh()` never uses `SendEnv`/`AcceptEnv` or command-line `VAR=val`.

### 7.5 The hook protocol

`docs/hook-protocol.md`, versioned. Newline-delimited JSON on the control socket (after `hello`/`register`) or stdio for a spawned child. Requests carry a deadline; a hook that misses it is `Silent`; a hook that returns `ok:true` with a missing required field is R0303.

```
engine → hook: { "id", "kind": "journal",   "op": "append", "entry": {...} }                        hook → { "id", "ok": true }
engine → hook: { "id", "kind": "inventory", "op": "list" }                                            hook → { "id", "ok": true, "hosts": [HostRecord] }
engine → hook: { "id", "kind": "execute",   "op": "run", "host", "instance", "body", "env", "secrets": {...} }   hook → { "id", "ok": true, "output": {...}, "facts": [...] }
engine → hook: { "id", "kind": "probe",     "op": "observe", "host", "probe" }                        hook → { "id", "ok": true, "fact": {...} }
engine → hook: { "id", "kind": "approval",  "op": "authenticators" }                                  hook → { "id", "ok": true, "authenticators": [ { "id", "human": bool } ] }
engine → hook: { "id", "kind": "approval",  "op": "challenge", "instance", "digest", "scope": { "kind": "plan"|"step"|"ack", "step" }, "context": { "cost", "plan", "host" } }
                                                                                                       hook → { "id", "ok": true, "challenge": "<binding's rendering>" }
engine → hook: { "id", "kind": "approval",  "op": "verify", "instance", "digest", "scope", "authenticator", "proof" }   hook → { "id", "ok": true, "verified": bool, "reason" }
engine → hook: { "id", "kind": "secrets",   "op": "resolve", "ref" }                                  hook → { "id", "ok": true, "value": "<secret>" }
engine → hook: { "id", "kind": "secrets",   "op": "deliver", "instance", "label", "value": "<secret>" }   hook → { "id", "ok": true, "accepted": bool, "receipt" }
engine → hook: { "id", "kind": "notify",    "op": "deliver", "level", "subject", "body" }             hook → { "id", "ok": true }
engine → hook: { "id", "kind": "scheduler", "op": "install"|"arm"|"rearm"|"disarm"|"present", "host", "artifact", "deadline" }   hook → { "id", "ok": true, "present": true|false|"unknown" }
```

Secrets cross the hook boundary in exactly four messages: `execute.run` outputs marked `secret` and `secrets.resolve` (toward the engine); `secrets.deliver` and `execute.run` `secrets` (toward a hook). Any other message carrying a `Secret` is R0305 and the value is dropped rather than sent.

### 7.6 Journal delivery

Chain and sign in the engine, then deliver to every declared sink synchronously; all must ack (or the `journal` declaration names which are authoritative). The write-ahead entry for a step is acked before its `do` runs. On sink refusal (R0304) the plan refuses to proceed and the refusal is itself journaled to whichever sinks still ack. A binding failing at runtime for any other reason is R0302.

### 7.7 Instance directories, backstops, drift

**Bootstrap.** Rue has no elevation mechanism. `<rue_root>` (root-owned `0755`), the `rue` group, `<rue_root>/instances/` (`2770` root:`rue`) and `<rue_root>/lock` (`0664` root:`rue`) are created once per run-capable target by the tenant's provisioning; `rue bootstrap <host>` verifies them via `Executor::bootstrap_state` and prints the exact commands for that OS family when something is missing, never running them. "Target bootstrapped" is a host-viability precondition (built-in probe `rue_root_ready`, R0407 if absent, reported by `rue doctor`).

**Instance directory.** `<rue_root>/instances/<instance-id>/` is created before the first step on every host with a run-capable executor (`local()`, `ssh()`, or a hook declaring `filesystem`), as the instance's `Owned` footprint there, mode `2770` group `rue`, every file inside `0640` group `rue`. It holds the stdin shim, `stage()`d files, `Modified` snapshots (size-capped, E0408/R0204), completion markers, a manifest of regions held on that host, the heartbeat file, and — only if the plan has a `:target` backstop — the artifact. Files are updated by write-to-temp-then-rename in the same directory, never in place, so a root cron artifact and a group-member engine can each supersede or remove what the other wrote. On hosts with no run-capable executor, markers, snapshots and manifests live in the controller's store; a `:target` undo there is E0407 at check and R0408 at runtime, and the verdict says which per host. Wrong modes refuse arming (R0406). The directory is removed when the instance closes or commits.

**Locks.** `<rue_root>/lock` is a host-wide lock (`flock(LOCK_EX)` on an `O_RDONLY` descriptor, so root and the ssh user share it; a named mutex with a group DACL on Windows), held by anyone reading or writing any instance's region manifest — and held across the *whole* of a region undo, from reading sibling manifests through the write. A per-instance lock in the instance directory protects that instance's markers and undo. Lock order is host, then instance, always. The tier-6 race stage exercises both.

**Backstop rendering.** A `:target` backstop is rendered by `rue-render` from the undo bodies of every covered step, in reverse order, into one standalone artifact in the host's declared artifact language (POSIX `sh`; PowerShell; Python under `uv` with PEP 723 metadata, on any OS; §4.5), with every path and value baked in (quoted per family), a deadline file, a fired marker, the trigger logic, and per-step completion markers: the artifact undoes only steps whose marker is present and removes a marker after undoing its step. The instance-directory layout the artifact reads and the engine writes (`deadline` and `heartbeat` as epoch seconds in text, `markers/<n>` with one `<kind> <path> <sha256>` line per file fact, `snapshots/<n>/<k>`, `manifest`, and the region marker lines `# rue-region <anchor> begin`/`end`) is the contract stated in `docs/DESIGN.md`. A non-file fact cannot be observed by a script and is undone as if intact. Installation is via the executor; arming writes the deadline; rearm rewrites only the deadline; disarm removes the artifact. The scheduler entry is the `backstop scheduler` binding's, and for PowerShell it invokes the script by `-EncodedCommand` or on stdin so execution policy never applies. The fired marker is read on the next engine contact and journaled `BackstopFired` (R0402).

**Drift.** Undo-time drift is handled by the step's policy (§5.2), identically by the engine and the artifact: `:clobber` restores or strips and leaves a `clobbered` marker the engine journals as `DriftClobbered`; `:defer` leaves the fact and a `drift` marker the engine journals as `DriftHeld`. The race between a fired artifact and an engine revert ends in the same place either way, under both locks, and is a tier-6 stage.

**Reconciliation.** At boot the engine lists every instance directory on every reachable host and compares with the store. A directory the store does not know about is *left in place* if it contains an armed, unfired artifact (journaled `InstanceDirOrphaned{armed: true}`, listed by `rue doctor`); only directories with no artifact or a fired marker are reclaimed automatically. `rue reclaim` is refused while an artifact is armed with its scheduler entry present (R0405); `rue reclaim --force --reason` is accepted when the scheduler entry is absent or the operator states the artifact has been read, journaled `Reclaimed{forced: true}`. `abandon` disarms an instance's backstops where hosts are reachable and otherwise leaves them armed and says so (`Abandoned{artifacts_left_armed}`); a later firing is `BackstopFiredAfterAbandon`, accepted and visible.

### 7.8 Reap pass and boot recovery

The reap pass (also `--once` for cron) observes expiry and lapses, retries `Stuck`, lapses `Pending`, reads fired markers, polls `handoff_done:` probes, re-sends notifications for every unbounded state, and drops `hold()` secrets past their bound. Boot recovery demotes `Applying` to `Reverting`, reestablishes `Held` resources, removes staged files for any instance not `Applying`, reconciles instance directories (§7.7), journals `SecretDropped{reason: :daemon_restart}` for every held secret, and enters **settle**: a start-up window during which no `wane` fires and no stuck retry runs until every `Held` footprint has been reestablished or lost and every owned footprint has been re-observed.

### 7.9 Two dry-runs

`rued --dry-run` is a **daemon mode**: no executors registered, `always()` accepted as approval, E0604 suspended, every apply journal-only, nothing reserved. `rue apply --dry-run` is a **request flag** against a real daemon: the plan is checked; its gates are evaluated (satisfiability, digest, rendered challenge — journaled, then `Approved{rehearsal: true}` without waiting for proofs) but not awaited; every step is journaled; no executor is called; umbras are not entered in the ledger and no exclusivity is held, so a rehearsal never blocks a real plan. Its verdict says "rehearsal: no reservation". Every "outside dry-run" in this document means the daemon mode.

### 7.10 Secret delivery in the engine

Delivery, placement and drop rules are §5.13. In the engine: the `hold()` acceptor keeps a value in process memory only; `rue reveal` delivers it once to the requesting identity; a daemon restart loses it and says so. `secrets.deliver` to a hook is the only message carrying a secret toward a hook other than `execute.run`'s `secrets` field.

### 7.11 Embedding SDKs

Any language that can open a socket or pipe and speak newline-delimited JSON can embed rue; an SDK is a convenience over the protocol. An SDK provides registration with kinds and protocol version; typed request/response for every hook op with deadline handling; the rule that secrets appear only in the four permitted messages, exposing `execute.run` secrets to a hook's `run` handler without ever placing them on a command line; and a conformance runner (`rue sdk-conform <command>`, Phase 4) it must pass before calling itself an SDK.

| SDK | Why | Shape |
|---|---|---|
| Rust (`sdk/rust`) | Reference; shares types with the engine via `rue-hook-proto` | Library crate |
| Elixir (`sdk/elixir`) | T4's host; a GenServer owning the socket, hooks as behaviours | Hex package |
| Python (`sdk/python`) | Simulators and glue | PyPI package, stdlib-only |
| Java (`sdk/java`) | The org's JVM libraries and any JVM host | Maven artifact, no framework dependency |
| .NET (`sdk/dotnet`) | Pairs with the Windows commitment (D-031); its conformance run executes on the Windows guest | NuGet package, no framework dependency |
| shim (`sdk/shim`, `rue-hook`) | For `sh` tenants and anything else: registers as a hook and dispatches each request to a configured command | Static binary, all three OS families |

Go and Node are recorded as gaps with a capacity reason, not non-goals: each is a common adopter's language, sequenced after the initial set. A Rustler NIF is a deliberate non-goal: in-process embedding couples the host's release to a Rust toolchain and lets a hook crash the host.

### 7.12 Threat model

| Threat | Status | Mitigation or acceptance |
|---|---|---|
| Compromised target disarms its own backstop | Accepted in v0 | A target can always kill its own scheduler. The verdict marks every `:target` backstop "self-enforced on <host>". An independent watchdog (separate machine, separate credential) is a v1 item (§11) |
| Compromised controller rewrites an unsigned chain from genesis | Mitigated when signing is declared | Chaining detects edits by anyone who cannot recompute the chain; only a key outside the engine's process defeats a fully compromised controller. The README says which guarantee each configuration gives |
| Hook trusted by name | Accepted with a declared boundary | Registration requires peer-credential identity and a registrar declaration; a hook can still lie about the world and the checker cannot tell |
| A process in group `rue` connects without a declared identity | Mitigated | Refused (R0503); group membership grants a connection, never an identity |
| A compromised embedded host process acts as an operator | Accepted with scope | It is a control client with a declared identity scoped by `operator_for`; every act is journaled under that identity |
| A compromised or careless admin force-reclaims a live backstop, or abandons an instance | Accepted, on the record | Admin must be declared; `--reason` required and journaled under the peer-credential identity; notify on every admin act |
| Replayed or re-targeted proof | Mitigated | Nonce, `params_hash`, `host_contract_hash`, `gate_hash`, scope in the digest (§5.11) |
| Requester approves itself | Mitigated | E0508 at load (gates); acks are the operator's own decision by design |
| Host role or static fact changed after approval | Mitigated | `host_contract_hash` in the digest; R0301 refusal |
| Secret leakage via journal, explain, argv, `ps`, artifact, hook | Mitigated | §5.13: never in entries; stdin preamble or staged mode-0600 file; only the four permitted hook messages |
| Held secret lost on daemon restart | Accepted, visible | `hold()` is memory-only by design; journaled at boot, distinct from a reveal |
| Another actor edits a footprint mid-plan | Mitigated | Post-`do` drift diff (R0201); undo-time drift policy (R0202) |
| Induced drift as denial of revert: an actor edits a `modified` fact so a `:defer` undo declines | Accepted per step, visibly | Mitigation is `drift: :clobber`; a `:defer` step under `:auto` is named in the verdict (`induced_defer`); `reach` ops cannot be `:defer` (E0410) |
| A foreign region on a shared fact blocks a region's damaged-marker fallback | Accepted, visible | Defer rather than clobber a sibling's region; the step is marked conditional |
| Clock manipulation on the target | Partially mitigated | Skew probe at arm (R0403); a target that later steps its clock backward delays its own backstop — accepted; `unless_heartbeat` is the trigger to use where that matters |
| Controller store loss or rollback at boot | Mitigated | Armed, unfired artifacts are never reclaimed automatically |
| Plan text tampered between `check` and `apply` | Mitigated | `apply` re-checks; `plan_content_hash` is in the digest |

### 7.13 Instance store and migration

The store carries a schema version; `rued` refuses to start on an unknown or newer version (R0502) and never silently migrates an older one: `rued migrate` is explicit, dry-runnable, refuses if the store is not owned by the daemon account, and is journaled `Migrated{from, to, by}` on the daemon's next start. Every release ships the previous release's store as an upgrade vector.

---
## 8. Acceptance tenants

Each tenant is a directory under `tenants/<name>/` with `.rue` files, hook stubs where embedded, an inventory, and `expected/` verdicts (JSON and prose) per host. Tenant names may appear **only** under `tenants/`, `docs/`, and test fixtures. They are the floor of capability the project is graded on, not its ceiling (§0.5).

Every tenant's site block declares an `operators` block (at minimum the requesting operator with `operator_for: :all` and one `admin: true`) and, where hooks are declared, a `hooks` registrar block. Every tenant's tests include one control connection from an undeclared identity (R0503) and, for hook tenants, one registration outside `may_register` (R0505).

### 8.1 T1 — break-glass access

Temporary plan. Must express: four channel ops — a service-posture drop-in as `owned` with a `derived` verify; a fenced block in a shared authorised-keys file as `region`; a management-controller account enable as `modified` with a `secret` output and `undo_locus: :controller` (an API host: no instance directory, controller-side markers); a console tunnel as `held` with `suspend`/`reestablish` and a rotated `secret`; a plan-entry gate via `approval: hook(:authority)`; `wane: 4h` with `renew_within`; a `:target` backstop `[after: 4h, unless_heartbeat: 60s]` covering the ssh-borne ops, armed after them (`reach` empty; late arming); scheduler presence and bootstrap as preconditions; `secrets deliver_to: [requester(), hook(:escrow)]`.

Expected verdict: temporary, reverts at wane 4h; reversible through 4; no knell; backstop covers 1–2 on target, installed before 1, armed after 2, engine-only until armed, self-enforced; steps 3–4 revert only while the engine lives; step 3 on an API host with controller-side markers; secrets from steps 3 and 4 delivered once at their steps.

### 8.2 T2 — cluster succession

Permanent plan ending in `commit()`. Must express: the promote ladder with rungs as three-valued guards; a "probes" rung with `force: never`; a fence rung as a `knell` whose guard is the fence driver's verified-off (`:unknown` forceable in manual mode, `:no` never) with a cost probe and `ack: :none` under `mode: :auto` / one human under manual; `mode: :auto` refusing all `force:` statically; per-guest steps after the knell with `refusal: :hold` (holding indefinitely under auto until `resume`, `recant` or `commit`); a second `knell` for destructive rollback of ahead datasets with a cost probe listing what is destroyed, reachable only on the manual path; succession log and placement as `append_only`; a resurrection gate's hold as `hold_via:` (platform slave-mode op, undo = release); a per-guest heir on another node as `deferred`, continued by `handoff-done`; exclusivity class per corpse (75 on contention); failback's written-bytes guard as `preflight` measured twice with an acknowledgement flag; a `repeat over:` per-guest loop that checks clean under strict mode.

Expected verdict: permanent, commits after the guests are up; reversible through the probes rung; point of no return at the fence rung, guard verified-off, cost = fence verdict, acknowledged by `:none` under auto and one human under manual; post-fence steps held indefinitely until an operator acts; second knell on the manual path with its cost listed; deferred steps named with handoff; no unsatisfiable gate; the auto plan proven to contain no human wait.

### 8.3 T3 — commit-confirmed firewall change

Permanent plan. Must express: a `region` change to a host firewall (default `drift: :clobber`; the tenant's file states the damaged-marker cost in a comment) with `reach ssh(host)`; `undo_locus: :target`; a backstop `[unless_confirmed: 10m]` installed and armed **before** the change; a reachability probe; `confirm()`; `commit()` last. Negative cases: the backstop moved after the change (E0401), the undo locus changed to `:controller` (E0401), `drift: :defer` (E0410), `commit()` not last (E0502). A Windows variant with a Windows Firewall rule declared by the tenant.

Expected verdict: permanent, commits at step 4; reversible through 1; backstop (unless confirmed within 10m) covers step 1 on target, installed and armed before it; if step 2 severs reachability the host reverts step 1 unaided *unless another instance holds a region in the firewall file, in which case the revert is deferred*; step 3 disarms; step 4 commits.

### 8.4 T4 — embedded: a temporary override in a reactive host

Temporary plan, embedded. Must express: `journal to: hook(:host_log)`, `inventory from: hook(:host_world)`, `execute via: hook(:host_actuate)`; an `operators` block declaring the host's identity with `operator_for: [:shed_load]` and a `hooks` block declaring the host as registrar for those three names; an op `shed_load` whose footprint is a group of actuator facts (`modified`, equivalence = reported state) with a restorative undo, `undo_locus: :controller` (no instance directory; "reverts only while the engine lives") and `drift: :clobber` (the host's model is convergent); a `wane`; a plan the host fires from a state-machine clause in `mode: :manual`, as operator, and `recant`s on state exit; a second variant with `drift: :defer` whose hand-flip leaves the instance `DriftHeld`, forced by the host through the control channel; a scope-violation test (R0504); request dry-run journaling only.

Expected verdict: temporary, reverts at wane; reversible through 1; no backstop; drift policy stated per step; `DriftClobbered` journaled in the first variant, `DriftHeld` then `--force=drift` in the second.

---

## 9. Phases

Each phase has deliverables, tasks, tests, acceptance, exit criteria, a "not proven" table and the rediscovery rows it seeds. A phase exits with an honest table, never an empty one. Every phase's exit criteria include the tenant test (§0.4) and a green seam guard.

### Phase 0 — Prove the core (prototype)

**Goal.** Establish that footprint kinds, undo loci, the refusal lattice, intent, backstop triggers, three-valued guards and the `reach` rule compose into a checker whose verdicts read correctly for all four tenants — before any Rust.

**Deliverables.** `proto/` in Haskell (preferred: `Category`/`Arrow` instances exist) or OCaml; the four tenants as prototype terms *and* as unparsed `.rue` text beside them; `docs/verdict-schema.json` v1; `docs/prior-art.md` from the falsification day.

**Tasks.**
1. Model `Fact`, `Tri`, `Kind`, `Footprint{shape, instance, anchor}`, `Op`, `Plan` with intent, `Guard`, `Refusal`, `Locus`, `Backstop{Trigger}`, `Reach`, `GateSpec`.
2. Implement `seq`, `par`, `reverse`, `reverse_from`, `knell`, `hold`, `confirm`, `commit`, `preflight`, `observe`, `assert`, `repeat`, `when`.
3. Implement the interference query over shapes; the refusal lattice; backstop coverage and the late-arming window; the `reach` ordering check.
4. Implement `check :: Site -> Requester -> Plan -> Verdict` producing the structured form and the prose, and `explain`.
5. Model the state machine from §5.9's five rules and generate the transition table.
6. Encode T1–T4 as terms.
7. Write T1–T4 as `.rue` text against §6; every construct the terms need must have a spelling and every spelling must appear in §6. Grammar gaps found here are fixed in §6 before Phase 2.
8. Encode the negative cases: E0401 (T3 with late arming), E0404 (auto with force), E0410 (`reach` with `:defer`), E0501 (both `wane` and `commit()`), E0502 (`commit()` not last), E0505 (permanent path without commit), E0506 (unbounded wait), E0507 (auto with human ack), E0508 (requester counted), E0509 (zero-human gate); the `repeat over:` loop checking clean under strict; a region step with a foreign region held asserting per-step `conditional`; two plans overlapping at `Pending` refused at request; a knell acked by its requester accepted while the same requester's entry-gate proof is refused; step-gate lapse under `:revert` and `:hold`; a permanent plan never reverting at any time and releasing umbras on commit.
9. Write `docs/verdict-schema.json` from the prototype's output.
10. Spend the falsification day (§1.3); record results.

**Tests.** Property tests for the laws in §5.5 (`reverse . reverse ≡ id` on knell-free plans; `reverse (seq a b) ≡ seq (reverse b) (reverse a)`; `par` admitted ⇔ pairwise umbra-disjoint). Goldens for the four tenants' verdicts (JSON and prose) and the negative cases.

**Acceptance.** Every tenant expresses without a special case; every negative case refuses with the intended code; the prose verdicts match §8.

**Exit criteria.** Acceptance met, or a written finding that the model is wrong in a specific way, with the fix folded into §5 and this phase re-run. Either outcome is a valid exit; silently proceeding is not.

**Not proven.** Footprint honesty at runtime; `:target` undo closure (a boolean flag here, analysis in Phase 1); verdict prose stability beyond four tenants; `hold_via:` (T2's resurrection gate is an ordinary `:hold` step with a footprint in the prototype; the form gets its own treatment in Phase 1).

**Rediscovery rows seeded.** `revert-composition-law`, `par-not-disjoint`, `reach-late-arm`, `auto-with-force`, `knell-no-cost`, `permanent-plan-reverts-at-wane`, `commit-before-last`, `intent-ambiguous-accepted`, `foreign-region-clobbered`, `pending-no-reserve`, `proof-scope-ignored`, `held-outlives-wane`.

### Phase 1 — rue-core and rue-render in Rust

**Goal.** Transcribe the proven calculus into pure, I/O-free crates with golden-tested verdicts and the state machine.

**Deliverables.** `core/`, `render/`; `docs/DESIGN.md`; verdict schema v1 frozen; the seam guard; repository scaffold and reaper tenancy on FreeBSD and Linux, with Windows tested under wine: every binary cross-built for `x86_64-pc-windows-gnu` and the whole suite run under wine on the Ubuntu reaper guest and in CI (`ci/test-windows.sh`). Services, named pipes, the Task Scheduler and ACLs are beyond what wine proves and are Phase 3's to test on a real machine.

**Tasks.**
1. Types per §5 with `serde` and the canonical encoding (length-prefixed, field-ordered, domain-separated) for journal hashing and the request digest.
2. `check`, `explain`, `render` as pure functions; interference as iterator joins shaped as the Datalog (§5.7); disjointness for `par`; anchors.
3. Closure analysis for `:target` undos (E0202) and secret-placement checks (E0209, E0210, E0211) over `Body`/`Prim` argument classes.
4. Refusal lattice; intent inference and its checks (E0501–E0505); backstop coverage, triggers by intent, `reach` ordering, install/arm (E0401–E0403, E0406); bounded waits by reachability (E0506); gate satisfiability, minimum humans, requester exclusion, zero-human (E0508, E0509).
5. The state machine from the five rules with an injected `now`; renewal windows; exclusivity; secret delivery at step completion.
6. Journal model: entry type, chain, `secret_labels`, signature slot.
7. `rue-render`: artifact text per OS family and artifact language (`sh`, PowerShell, Python under `uv`) for the three triggers, completion markers, drift as §5.2, per-family quoting (E0109); the `sh` and Python artifacts executed in tests against a temporary instance directory.
8. Diagnostics type with codes, spans, expected/found, nearest-name.
9. `tools/lint-seam.sh`, `tools/seam-denylist.txt`, `tools/check.sh` reporting all failures.
10. Fuzz: a plan generator over random ops/footprints; the laws; `check` never panics.
11. Repository scaffold (§12): `bitbucket-pipelines.yml`, `ci/build-target.sh` for all five targets, pinned image and `rust-version`, `.reaper.toml` with FreeBSD and Linux guests, the Linux guest running the Windows suite under wine; the pipeline's wine step; first tag `v0.0.1` deploys core-only artifacts and the mirror reflects it.

**Tests.** Tier 1 units per rule (every E-code has a triggering and a non-triggering test); Tier 2 goldens for the four tenants' verdicts (JSON byte-identical after canonicalisation; prose golden); property/fuzz for the laws; Tier 3 seam guard self-test.

**Acceptance.** Verdict JSON matches Phase 0 byte-for-byte after canonicalisation; every E-code exercised; seam guard passes and its self-test fails on a planted word; `cargo clippy -D warnings` clean on all five targets; `reaper test` green on both registered guests, the Windows suite under wine included.

**Exit criteria.** Acceptance met; schema v1 tagged; "not proven" published.

**Not proven.** Anything about the world: honesty, executors, sinks, arming. Any surface syntax. PowerShell artifacts are rendered and golden-tested, executed nowhere (no gate host runs PowerShell); the `sh` and Python artifacts are executed on FreeBSD and Linux gate hosts only. The darwin binaries are cross-built and packaged, executed and signed nowhere. macOS as a host is proven only by an artifact golden. Positions the crates take where §5 is silent, for the owner: the instance-directory layout in `docs/DESIGN.md` (epoch seconds in the deadline and heartbeat files, never an mtime; `<kind> <path> <sha256>` marker lines; the `# rue-region` marker lines; snapshots by footprint index); a non-file fact under a computed `:target` undo is undone as if intact; a fact reference in a covered undo is not bakeable this unit though closure admits it; a control character inside a shell quote is E0109; closure (E0202) treats a plan parameter and a host-record field as bakeable into a target-side artifact, and an earlier step's output as never bakeable, since nothing in §7.7 persists an output to the target; E0206 is decided only as a structural re-run (a `reestablish` primitive equal to one of the op's `do`), the "reachable from" rule needing an op reference bodies do not carry; E0211 is decided for static hosts only, a `:controller` step and a bound host being unjudged at check; E0411 is not decided at all, the site not declaring sinks; E0406 cannot arise, installation preceding the first covered step by construction.

**Rediscovery rows seeded.** `chain-skips-prev-hash`, `expiry-at-boundary-open`, `secret-in-explain`, `knell-reverse-allowed`, `closure-uses-controller-fact`, `secret-in-run-string`, `secret-in-target-undo`, `reach-defer-drift-accepted`, `covered-step-before-install`, `temporary-after-not-wane`, `wait-unbounded`.

### Phase 2 — The surface

**Goal.** `.rue` files check and explain through the front end; diagnostics are a golden-tested contract.

**Deliverables.** `surface/`; `rue check/explain/render/fmt`; the four tenants as real `.rue` files; `docs/LANGUAGE.md`.

**Tasks.**
1. Lexer, parser with recovery, lossless tree.
2. Expression grammar and evaluator with `Unknown` and `Secret` propagation; kinds (E0107); E0108.
3. Totality fence (E0106) with teaching text.
4. Resolver: imports, namespacing, templates, clause dispatch per host, roles→slots with priority and deterministic ties, protocols with paired inverses, value flow and scoping (E0110, E0114), `repeat over:` set-valuedness (E0113).
5. `site` block: every binding kind, `operators` with `admin:`, `hooks` registrars (E0604, E0605), `max_wait`, `skew_tolerance`; parsed and validated, not executed; E0601–E0603, E0606 at check where statically known.
6. `miette` diagnostics; golden corpus for every code in §6.7.
7. `rue fmt` round-trips every tenant file byte-identically.
8. Tenants rewritten as `.rue` with hook stubs satisfying the front end's contract checks.

**Tests.** Parser goldens (every construct, every error); per-rule semantics; e2e `rue check tenants/<t>/plan.rue --host X --json` equals `expected/`; fmt idempotence; diagnostic goldens.

**Acceptance.** Four tenants check from `.rue` with verdicts identical to Phase 1 goldens; every diagnostic golden-tested; `fmt` idempotent; a planted closure yields E0106 with the teaching text. **Performance:** `rue check` of a 200-step plan against a 1,000-host inventory under 2 s on the CI image and 5 s on a FreeBSD guest, enforced by a benchmark test.

**Exit criteria.** Acceptance met; `LANGUAGE.md` complete enough that a reader can write T3 from it without this roadmap.

**Not proven.** Execution; bindings beyond parse-time validation; editor tooling; performance on Windows and beyond 1,000 hosts. What the grammar admits beyond the constructs the tenants use (`defprim`, roles and slots, protocols, the builtins, operators in guards) is proven by the parser corpus, the resolver's unit tests and the diagnostic goldens, not by a tenant. Positions the front end takes where §6 is silent, stated in docs/LANGUAGE.md for the owner: the site derivation rules (transports from `execute via:` with `[ssh]` the default, the hook's transport named on the binding, acceptors from `secrets deliver_to:`, the preamble from the record or its filesystem, the requester from `--as` or the first identity); one fact-shape rule (`a.b(x)` is `a:b:<x>`, names verbatim, a runtime value as `{name}`); reference classification through the call site; no `undo:` line means no undo; E0208 from `idempotent: true`; `file` admitted for an inventory beside `rue_toml`, `local` for a journal, `launchd` for a scheduler; a `repeat over:` list set-valued unless a literal list repeats a member; E0107 decided for declared defaults and plan options only; E0114 for output secrecy only; an implicit parameter is any free name in a body.

**Rediscovery rows seeded.** `unknown-compare-allowed`, `closure-accepted`, `clause-on-runtime-fact`, `slot-order-nondeterministic`, `fmt-loses-comment`, `items-after-commit` (carried as core's `commit-before-last-core`; the surface adds no second check), `implicit-operator-accepted` (carried as `hook-without-registrar-accepted` and `journal-optional`, the site rules the front end decides; an operator's implicitness is E0604, whose row is the same guard).

### Phase 3 — Engine, executors, standalone daemon

**Goal.** Plans apply, revert, expire, commit and recant against real hosts with write-ahead journaling, chained sinks, target-standalone backstops and drift detection. T1 and T3 run end to end.

**Deliverables.** `engine/`, `bindings/` (generic only), `rued` on all three OS families, the full `rue` CLI; `docs/hook-protocol.md` v1, `docs/control-protocol.md` v1; `docs/TESTING.md`; e2e harness under reaper; `sim/` (minimal).

**Tasks.**
1. Instance store: locked, atomic, versioned; boot recovery and settle (§7.8); `rued migrate` (§7.13).
2. Executors `local()` and `ssh()` with a transport seam and error-injecting fakes; the `Silent` rule; capabilities; the stdin-preamble shim.
3. Footprint snapshot before `do`; post-`do` drift diff (R0201); undo-time drift policy with `DriftHeld` and `--force=drift`; the foreign-region condition; region anchors.
4. Instance directories on run-capable hosts and controller-side markers for the rest; modes and setgid group; write-then-rename; host and instance locks with the region undo under the host lock; `Executor::bootstrap_state`, `rue_root_ready`, `rue bootstrap` as verify-and-print; `rue doctor` mode and bootstrap checks; `stage()` lifecycle and recovery.
5. Backstop installation, arming, rearming and disarming through the `backstop scheduler` binding; generic `cron()` and `task_scheduler()` in `rue-bindings`; heartbeat; skew probe; fired-marker journaling; reconciliation that never reclaims an armed artifact; `rue reclaim` with its override; `abandon` disarm-first.
6. Journal: chain, optional signing, multi-sink synchronous delivery, `file()` and `stdout()` sinks, refusal on no ack; `rue journal verify`.
7. The control channel: socket modes, `hello`, peer-credential identity mapping, declared operators with `:all`, `:socket_owner`, `admin:` and `subscribe`, scopes, R0501/R0503/R0504/R0506; hook registration after `hello` with registrar checks (R0505); stdio children as `:socket_owner`; identity events journaled.
8. Gates: approval binding contract with `human` flags; request, step and ack digests; accumulating proofs; windows; `always()` refused outside daemon dry-run; `rue approve --step`, `rue ack`, `--ack` up front.
9. Intent at runtime: `commit()` item and verb, `Committed`, backstop disarm-first on commit; `confirm`; R0102.
10. Waits and holds: `Waiting`, `Held`, `Deferred`, `DriftHeld`, `Stuck` with the state-holding table enforced in the ledger; umbra reservation at `Pending`; rehearsals reserving nothing; one effective bound per wait, `wane` always winning, `on_lapse`; `resume`, `handoff-done` (verb and probe), `abandon`; exit codes 3–8; notifications for unbounded states re-sent each pass.
11. Secrets: delivery at step completion through `secrets deliver_to`; `requester()` and `hold()`; `rue reveal`; drop rules; the four permitted hook messages (R0305).
12. Windows: `rued` as a service; named-pipe control channel with group DACL and client-SID identity; `%ProgramData%\rue`; `local()` and `ssh()`; PowerShell renderer with `task_scheduler()`.
13. `sim/` minimal: virtual clock, seeded events, the invariants of §10.3, a shrinker — enough for T1 and T3.
14. e2e under reaper: T1 against a real sshd and the tenant's own BMC simulator fixture; T3 against a real pf/nftables host **and** a Windows Firewall host where the plan really severs ssh and the host really reverts, and commits when confirmed; revert-under-kill (SIGKILL mid-apply; boot; demotion and revert); backstop-under-daemon-death; backstop-vs-recant race under both locks; a hand-edited `modified` footprint under `:defer` entering `DriftHeld` and proceeding with `--force=drift`, and restored under `:clobber`, identically whether the engine or a fired artifact ran the undo; a planted out-of-footprint write refused; a planted journal deletion detected; `rue doctor --canary` proving a real backstop fired.

**Tests.** Tier 1 units per driver with fakes; Tier 4 lifecycle truth table (every state × every event, generated from §5.9); Tier 5 real-host e2e; Tier 6 kill/death/race battery; Tier 7 minimal simulation; rediscovery.

**Acceptance.** *(Amended 2026-09-08, by the owner's decision of 2026-09-07: Windows stays under wine this phase.)* Everything in task 14 passes on the FreeBSD and Linux guests, and every R-code in Appendix D has a test that raises it. The Windows half is built for `x86_64-pc-windows-gnu`, unit-tested under wine, and proven no further this phase: `rued` as a service, the named-pipe channel with its access-control list and client-SID identity, the Task Scheduler backstop and the Windows Firewall variant of T3 wait for **Phase 3W**, a named later item that runs task 14 on a real Windows guest against no fakes. Nothing about the Windows design is left unwritten; what is missing is a machine to run it on.

**Exit criteria.** Acceptance met; `TESTING.md` published; tag v0.1.0.

**Not proven.** `unless_heartbeat` under real network partition (a vnet stage in Phase 5); schedulers other than cron and Task Scheduler; multi-controller contention. Task 4's *instance* lock is not implemented: the store's own lock admits one daemon, the artifact holds the host lock for its whole run and the engine holds it across any region undo, and two controllers on one host is Phase 5's question; the reasoning is in docs/DESIGN.md, and a lock nothing takes would be worse than none. On Windows, what wine cannot show: the service-control manager (the dispatcher and its stop handler are unit-tested by argument, and no manager has started rued), the access-control list as the kernel enforces it against a client that should be refused, the Task Scheduler, and PowerShell as `local()`'s shell. Wine *does* carry the named pipe end to end, list and all, and names the client from its own SID, so the identity model is proven on Windows and only its enforcement against a stranger is not. On macOS, `launchd()`, which is written and unit-tested and has installed no job.

**Rediscovery rows seeded.** `apply-before-journal-ack`, `drift-outside-footprint-ignored`, `undo-restores-over-actor`, `backstop-armed-after-reach-op`, `renew-commits-before-rearm`, `secret-journaled`, `stuck-swallowed`, `silent-executor-ok`, `settle-fires-wane`, `artifact-engine-drift-disagree`, `staged-file-survives-crash`, `same-anchor-two-plans-allowed`, `driftheld-releases-umbras`, `boot-reclaims-armed-artifact`, `manifest-read-unlocked`, `region-undo-releases-host-lock-early`, `unbootstrapped-target-dir-created`, `wrong-modes-armed`, `target-undo-no-filesystem-sticks`, `manifest-rewritten-in-place`, `driftheld-reverted-at-wane`, `bootstrap-runs-commands`, `register-before-hello`, `admin-verb-without-admin`, `hook-registration-unjournaled`, `force-reclaim-without-reason`, `rehearsal-reserves-umbras`, `hold-under-auto-refused`, `stuck-inescapable`, `abandon-leaves-artifact-unjournaled`, `deferred-no-way-out`, `temporary-heartbeat-refused`, `secret-held-past-revert`, `commit-leaves-backstop-armed`, `hook-registered-by-undeclared`, `scope-violation-allowed`, `ack-without-token`, `host-contract-change-ignored`, `secret-in-forbidden-hook-message`.

### Phase 4 — Embedding

**Goal.** A host process registers hooks and runs rue plans as its own effects; T4 runs end to end with the tenant's stubs; T2 runs against a pseudo-cluster via hooks.

**Deliverables.** The `sdk/` set (§7.11), each passing the conformance suite; `rue sdk-conform`; `tenants/t4/` with a minimal reactive host stub; `tenants/t2/` with hooks over a jail-based pseudo-cluster; hook protocol v1 frozen.

**Tasks.**
1. `rue-hook-proto` and the Rust reference SDK.
2. Conformance suite covering every hook kind, deadlines, `Silent`, R0303, the four secret messages, and the replay-resistance contract for approval hooks.
3. Elixir, Python, Java and .NET SDKs and the `rue-hook` shim, each with a conformance run in CI (the .NET run on the Windows guest); the shim built for all three OS families.
4. T4: host stub with a state machine that fires `shed_load` on enter and `recant`s on exit, connecting as a declared operator scoped to `:shed_load` with a `DriftHeld` subscription, registering its three hooks as a declared registrar with one registration outside `may_register` refused; both drift variants; a scope-violation test; a request dry-run.
5. T2: hooks over the pseudo-cluster; the permanent promote plan ending in `commit()`; the auto variant's post-fence holds continuing via `resume`; a deferred heir via `handoff-done`; both knells with guards, costs and acks; exclusivity on contention; failback preflight measured twice.
6. Multi-sink journal with one hook sink refusing: the plan refuses to proceed and the refusal reaches the other sink.

**Acceptance.** *(Amended 2026-09-11, by the owner's decisions of 2026-09-09.)* T4 and T2 pass; a hook returning `ok:true` with a missing field is refused; a hook missing its deadline is `Silent` and the plan refuses; the same `.rue` text checks identically standalone and embedded. Three conditions stand beside it. The SDK conformance runs execute on the Ubuntu reaper guest and in the pipeline, one image per SDK, and never in the local gate, whose workstation carries none of their toolchains. The .NET run is on Linux: task 3 pairs it with the Windows guest, which Phase 3W owns. T2's pseudo-cluster is base `jail(8)` on the FreeBSD guest, driven by a text of T2's shape beside its test (`tenants/e2e/tests/succession.rs`): node-b is the guest, node-a is reached only through the cluster driver's hooks, node-c is on the console, and the rollback knell acts on a real ZFS dataset. `tenants/t2/plan.rue` stays the checked artifact, `cbsd` calls and all.

**Exit criteria.** Acceptance met; hook protocol v1 frozen (`docs/hook-protocol-v1.json`, pinned by the `hook-proto-frozen` gate phase); tag v0.2.0.

**Not proven.** Any real reactive host: T4's is the tenant's stub on the Elixir SDK, on one OS family (the Ubuntu guest, where Elixir is provisioned). Hook round-trip performance under load. NIF embedding (deliberately not built). .NET on Windows (Phase 3W). T2's own text: its `cbsd` calls run nowhere, and what runs is a text of the same shape over jails. The non-Rust SDKs have no package-native test suites: `rue sdk-conform` drives every op of every kind through each one's own serve loop, and nothing else tests them, the Java SDK's hand-written JSON codec included. Java's packaging is proven only by the pipeline's maven step; the reaper guest compiles with `javac`. A `modified` fact that is not a file, over `ssh()`: ssh reads files only, so such a fact -- T2's `guest.state(g)` -- reads as absent both when it is marked and when it is checked, and drift on it is never seen; T2's guests are observed through probes, not through drift. The same read treats a failed read, a dropped connection among them, as absence, on every executor. Both predate this phase and wait for the owner. A failed step's undo against an object it did not make: a failed step is undone at once (5.9), so an undo that removes by name -- T2's `jail -r`, its `cbsd bstop` -- removes a same-named object that predates the plan, and `undo_pre` guards only facts the executor can read, which `guest.state(g)` over ssh is not; the T2 stage faults the placement service rather than planting such an object. A step inside a `repeat` that the backstop artifact undoes: the artifact is rendered per step, not per iteration. An instance applied inside a `repeat` under store schema 1 undoes without its variables, as schema 1 did; `rued migrate` says so and cannot recover them.

**Rediscovery rows seeded.** `hook-missing-field-accepted`, `hook-silent-as-ok`, `embedded-verdict-differs`, `sink-refusal-not-propagated`. **Rows added during the phase** (each verified by `sh tools/rediscovery/run.sh`): `hook-host-loses-its-rue-root`, `secret-printed-by-debug`, `hook-killed-not-closed`, `subscriber-loses-events-mid-verb`, `secret-unresolved-runs-blank`, `secrets-file-world-readable`, `hook-inventory-read-from-file`, `shim-repairs-a-reply`, `nonfile-fact-cannot-drift`, `nonfile-fact-unwatched`, `journal-sinks-truncated`, `rehearsal-blocks-the-real-plan`, `hook-protocol-changes-silently`, `controller-never-reaches-a-hook`, `hook-action-on-local-checks-clean`, `probe-without-run-body-checks-clean`, `imported-probe-declared-qualified`, `open-import-probe-undeclared`, `knell-cost-shown-by-name`, `handoff-probe-unchecked`, `static-probe-observed-with-empty-body`, `contract-change-midplan-crashes`, `contract-change-while-waiting-crashes`, `static-probe-unchecked`, `ack-verified-for-the-operator`, `ack-challenge-unobtainable`, `call-arguments-lost-in-the-body`, `repeat-undo-without-its-item`, `repeat-shape-taken-literally`, `repeat-iterations-share-a-snapshot`, `nested-repeat-skips-its-second-pass`, `older-store-reported-as-newer`.

**What the phase found by running.** T2 had checked clean for four phases and could not have executed a step past its probes. Running it found: a controller that could reach no hook; plans the checker passed that no executor could perform (E0608); a resolver that declared probes under names nothing referenced; a knell's cost shown as a label; static probes never frozen; an acknowledgement proved against the operator rather than the authenticator, and its challenge unobtainable; a call's arguments lost on the way into the op's body; and a repeat's iterations undone with no variable, sharing one snapshot, and skipped under nesting. Each is fixed with a test and a row; the last is store schema 2.

### Phase 5 — Tooling, drills, and the long tail

**Deliverables.** `tree-sitter-rue`; an LSP (`tower-lsp`) with hover = footprint/undo/locus/drift and diagnostics; `rue explain --html`; drill mode (scheduled apply-and-recant against a canary with a chain-verified attestation); the `unless_heartbeat` partition stage; the complete `sim/`; upgrade vectors (previous release's `.rue` files and store must still check and migrate, or fail with a stated migration).

**Acceptance.** Editor highlighting and hover on all tenant files; drill attestation journaled and verified; partition stage passes; v0.1.0 tenant files under v0.3.0 pass or emit a migration diagnostic naming the change.

**Exit criteria.** Tag v0.3.0; public README with §1.2 as its prior-art section; the name sweep recorded.

---

## 10. Testing portfolio

### 10.1 Tiers

| Tier | Where | What |
|---|---|---|
| 1 | workstation | Pure units: every core rule, every diagnostic, every driver against fakes |
| 2 | workstation | Goldens: verdict JSON and prose, diagnostics, `explain`, `fmt` round-trip, canonical encodings |
| 3 | workstation | Source-as-data guards: seam denylist; every E-code in §6.7 has a positive and a negative test and appears in `LANGUAGE.md`; every verb and flag in `--help` has a section; every schema field is produced by a golden and every golden validates |
| 4 | workstation | Lifecycle truth table generated from §5.9 (state × event), refusal lattice table, interference table, fuzz |
| 5 | VM guest | Real executors and hosts; backstop install/fire; T1, T3 |
| 6 | VM guest | Kill/death/partition/race battery; T2 pseudo-cluster stages |
| 7 | VM guest | Seeded simulation over the shadow world, invariants after every event, shrinker |

### 10.2 The rediscovery table

`tools/rediscovery/table.tsv`: one row per protection the project has paid for — a patch that reverts it, the tier that must fail, and for tier 7 the seed and step count that reaches it. `sh tools/rediscovery/run.sh --tier N` applies each row's patch and requires the suite to fail. Run before a milestone is trusted, never automatically. Every rule stated as MUST in this document gets a row when it is implemented.

### 10.3 Invariants for tier 7

After every simulated event, the shadow model and the engine must agree on: (1) the set of applied steps; (2) the set of owned/region footprints present on each host; (3) the stuck set; (4) the journal chain verifies; (5) no `Secret` appears in any sink; (6) no `wane` fired during settle; (7) a `reach` op never applied before its backstop armed; (8) for every drift event, the end state is identical whether the engine or the artifact ran the undo; (9) no `Secret` ever appears in an argv, a shell history, or an artifact; (10) no covered step ran before its artifact was installed; (11) no staged file survives an instance that is not `Applying`; (12) a region is never clobbered while another active instance holds a region on the same fact; (13) `Pending` reservations and the holdings of every held state are present in the ledger for exactly the instances in those states; (14) a proof made for one scope never satisfies another; (15) boot reconciliation never removes a directory containing an armed, unfired artifact; (16) no `Waiting`, `Held` or `Deferred` state outlives its effective bound, except `DriftHeld`, `Stuck`, and `Held`/`Deferred` in a permanent plan; (17) a region undo never observes a sibling manifest change between its decision and its write; (18) no control-channel act is performed by an undeclared identity or outside its scope; (19) a permanent plan is never reverted by time; (20) a committed plan's backstop never fires.

---

## 11. Open questions (decide when their phase starts)

| Question | Phase | Notes |
|---|---|---|
| Multi-controller: refuse, or a lock protocol over a shared fact? | 5 | v0 is single-controller per host; document the refusal |
| Byte-equality fallback for probes without a declared equivalence: allow with a warning, or require? | 2 | Lean: allow with a warning-class diagnostic |
| Sidecar vs spawned-child default for `rued` hooks in docs | 4 | Both via the same protocol |
| `winrm()` executor as a generic built-in, or OpenSSH-for-Windows only? | 3 | Lean: ssh only in v0 |
| Windows service wrapper: `windows-service` crate (msvc) vs a gnu-target shim | 3 | **Settled 2026-09-08:** the `windows-service` crate on the gnu target. Its dispatcher and stop handler are unit-tested by argument under wine; whether a real service-control manager accepts the gnu build is Phase 3W's to answer |
| **Phase 3W** — task 14 on a real Windows guest: `rued` as a service under the service-control manager, the named pipe's access-control list and client-SID identity as the kernel enforces them, the Task Scheduler backstop armed and fired, the Windows Firewall variant of T3, and PowerShell as `local()`'s shell | 3W | Named by the Phase 3 acceptance amendment of 2026-09-08. Everything it needs is written and built for `x86_64-pc-windows-gnu`; what is missing is the machine |
| `elevate via:` binding (sudo/doas/runas) so `rue bootstrap` could act, not only verify | v1 | v0 is free of elevation |
| Independent backstop watchdog (separate machine, separate credential; reads deadlines, re-asserts or pages) | v1 | Accepted out of v0 (§7.12) |
| Editor tooling before or after v0.1.0? | 5 | After; keep the grammar settled first |
| Python artifacts: `uv` and a cached interpreter as a bootstrap precondition on every OS; `uv run --offline` at fire time on a locked-out host; pre-warming at arm; what `rue bootstrap` prints when they are missing | 3 | The artifact is standard-library-only and the tests run it under `uv run --offline --script`; the target-side preconditions are the scheduler binding's and the bootstrap probe's to check |
| macOS as a controller: a Mac to execute, smoke-test, sign and notarize the cross-built darwin binaries; the launchd daemon; TCC and Full Disk Access for the scheduler job | 3 | The binaries are cross-built from Linux with zig and no SDK (§12) and ship unexecuted and unsigned until then |

---

## 12. Operational track (parallel to the phases)

- **Name sweep**: crates.io, PyPI, npm, GitHub for `rue`, `rued`, `rue-core`; record in `docs/prior-art.md` before anything public.
- **Repository**: primary on Bitbucket (`axonibyte/rue`), mirrored to GitHub by the `doMirror` pipeline step on every push (`git clone --mirror`, `git push --mirror`; repositories and deploy keys assumed provisioned). GitHub description: `[ Mirror ] A language for provably reversible operations`. License BSD-2-Clause, copyright Axonibyte Innovations, LLC.
- **Pipeline**: `bitbucket-pipelines.yml` in the sibling projects' shape — pinned `rust:<ver>` image and `rust-version`; `doMirror` first on every branch and tag; `doFetchAndTest` (cargo fetch `--locked`, `fmt --check`, `clippy --workspace --all-targets -- -D warnings`, `test --release`, shellcheck, the seam guard, service-install tests); parallel `build*` steps for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-freebsd`, `aarch64-unknown-freebsd`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-gnu` — all binaries on all targets, each arm running `cargo clippy` for its target before building — via `bash ci/build-target.sh <triple>` producing `dist/*`; the darwin targets cross-link with `cargo-zigbuild` against zig's bundled libSystem and no macOS SDK, which holds as long as no crate in the darwin graph links an Apple framework: `tools/lint-darwin-deps.sh` (a gate phase) fails on any such crate, TLS is `rustls` with `ring` and `webpki-roots` (never native certificates), and local-time-zone crates stay out; `doDeploy` on tags only: refuse if the tag disagrees with the workspace version, write `.sha256` sidecars, upload to Bitbucket Downloads with `BB_PUB_SECRET`. FreeBSD targets cross-link with `cargo-zigbuild` (amd64) and a pinned nightly `-Z build-std` (aarch64). `rustls` mandatory, `native-tls` banned. A released FreeBSD or Windows binary is smoke-tested on the oldest release it targets before it is announced.
- **reaper tenancy**: `.reaper.toml` at the root from Phase 1; guests `freebsd-15.1` and `ubuntu-26.04`, the latter also running the suite on the Windows target under wine (`ci/test-windows.sh`); `[build]` runs `cargo build --locked --workspace --all-targets` and, on the Ubuntu guest's container, the whole gate and the wine suite, against declared caches; from Phase 3, `[run]` executes on the guest itself (`exec = "host"`: the container carries neither sshd, nftables nor the capability for them) and is the tier 5 and 6 harness (`tenants/e2e/run.sh`) over a pinned 1.97.1 toolchain rustup installs into a cache (the FreeBSD port's rust is a minor behind the workspace); no pipes in any `cmd`. `reaper test` is the pre-push loop; CI is the independent re-proof.
- **Docs**: `README.md` (first screen: the claim, the honesty caveat, the prior-art table, which journal configuration gives which guarantee), `DESIGN.md`, `LANGUAGE.md`, `TESTING.md`, `hook-protocol.md`, `control-protocol.md`, `verdict-schema.json`, `prior-art.md`, this file.
- **Versioning**: SemVer for the crates; verdict schema, hook protocol and control protocol versioned independently; `.rue` and store upgrade vectors from v0.1.0 onward.

---

## Appendix A — Verdict prose grammar

```
<plan> on <host>: <intent>; <reversibility>; <hold>?; <knell>?; <gate>?; <stepgate>*; <backstop>?; <conditional>*; <controller-only>?; <hosts>*; <dispatch>; <may-conflict>?; <unresolved>?.
intent          := ("temporary; reverts at wane <D>" | "permanent; commits at step N" [", held indefinitely at step M until an operator acts"] [", undo fires by construction"]) [", revert can be induced to defer at step K"] | "rehearsal: no reservation"
reversibility   := "reversible through step N" | "not reversible past step 0" | "fully reversible (N steps)"
hold            := "step N holds on refusal (human required)" | "step N holds on refusal (until resume, recant or commit)"    -- the second under mode: :auto
knell           := "step N is a point of no return" [", guard <guard>"] [", cost C"] [", acknowledged by G"] ["; step M reversible back to step N"]
gate            := "gate satisfiable; minimum N distinct humans" | "gate satisfiable with no human (allowed)"
stepgate        := "step N gated by G" [", satisfiable by wait alone at +D"]
backstop        := "expiry backstop (<triggers>) covers steps A–B on the target, installed before step A, armed <before step A | after step B>" [", engine-only for steps A–B until armed"] [", fires within ~<granularity> after the deadline"] [", self-enforced on <host>"] [", drift: <per-step policy>"] [", snapshots on target (cap <N>)"]
conditional     := "step N reverts unaided unless <condition>; then deferred"
controller-only := "step(s) X, Y revert only while the engine lives"
hosts           := "step N touches <hosts>" | "step N touches a host bound at runtime" | "no instance directory on <host> (API executor); markers on controller"
dispatch        := "clause dispatch from inventory" | "clause dispatch assumed from inventory (static probe unevaluated offline)"
may-conflict    := "may-conflict between steps A and B on <fact shape> (strict: refused)"
unresolved      := "unresolved binding(s): <names>"
```

## Appendix B — `explain` line format

```
 N. <op>[(<args>)]   locus=<L>   refusal=<R>   drift=<D>   undo=<one-line undo or "NO UNDO — knell, cost <C>">   undo_locus=<UL>   [gate=<G>]   [ack=<A>]   [deferred → <handoff cmd>]
```

Secrets render as `<secret:label>`. Undo lines are printed before the step runs in `apply` output, in the journal's `Applying` entry, and in `explain`. A `region` op with `drift: :clobber` prints its damaged-marker cost.

## Appendix C — Minimal `HostRecord` contract

```json
{ "name": "db-01", "address": "10.0.4.11", "os": "freebsd", "roles": ["db", "primary"], "reach": ["ssh"], "facts": { } }
```

`name` unique and non-empty; `address` non-empty; `os` from a declared vocabulary the site may extend (`windows` is the PowerShell family; every other name is POSIX); `roles` a set; `reach` non-empty; `facts` free-form, available to clause dispatch (when `static`) and guards; `artifact` optional, `sh`, `powershell` or `python`, the language the host's backstop artifact is rendered in, the native shell when absent (§4.5); `rue_root` optional, where rue keeps this host's instance directories (§7.7), the family's default when absent (`/var/db/rue`, `C:\ProgramData\rue`).

## Appendix D — Runtime codes

Instance and locking: `R0101` exclusivity held (exit 75) · `R0102` verb not admitted by the plan's intent · `R0103` `recant` on `DriftHeld` without `--force=drift` · `R0104` `hold(until: :wane)` on a permanent plan with no site `max_wait`.

Footprints: `R0201` footprint violation observed · `R0202` drift observed; step policy applied · `R0203` cross-plan umbra overlap with an active or pending instance · `R0204` target snapshot cap exceeded.

Gates and bindings: `R0301` host contract changed since request (proofs invalidated) · `R0302` binding failed at runtime · `R0303` hook contract violation · `R0304` sink did not acknowledge · `R0305` a `Secret` in a hook message other than the four permitted (value dropped).

Backstops and targets: `R0401` backstop scheduler absent · `R0402` backstop fired (informational) · `R0403` target clock skew beyond tolerance at arm · `R0404` backstop rearm failed (renewal refused) · `R0405` reclaim refused (armed artifact, scheduler present) · `R0406` instance directory modes wrong (arming refused) · `R0407` target not bootstrapped · `R0408` `:target` undo on a host whose executor reports no filesystem, detected before `do` (step refused, prefix reverts).

Protocol and store: `R0501` control protocol version mismatch · `R0502` store schema version unknown (run `rued migrate`) · `R0503` control client identity does not match a declared operator · `R0504` act outside the client's `operator_for` scope · `R0505` hook registration from an undeclared registrar or outside its `may_register` · `R0506` admin verb attempted by a non-admin identity.
