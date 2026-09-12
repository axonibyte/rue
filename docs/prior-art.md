# Prior art: the falsification sweep

The claim under test (docs/ROADMAP.md section 1.1): rue is the first plan
language in which "this can be undone" is a compile-time verdict rather than
a comment. Section 1.3's standing order is that before Phase 0 exits, one
person spends a day trying to break the claim and records what was found.
This is that record. Swept 2026-09-06 by web search over the roadmap's
section 1.2 table plus adjacent fields it did not name; each entry states
what was checked and whether the claim survives as written, survives
narrowed, or falls.

## Verdict

The claim survives, narrowed in one place. Two bodies of work decide
reversibility offline and were not in the roadmap's table: action
reversibility in AI planning, and the compensation calculi. Neither is a
language for operations against hosts, and neither states the things rue's
verdict states (undo locus, cost, arming order, who must act, how long); but
"compile-time verdict on undoability" is not new in the abstract, and the
claim should say "for operations against a world, from declared footprints"
rather than imply the idea is unprecedented. The roadmap's section 1.1 is
the owner's to reword; the sentence proposed is in the last section.

## Candidates and deltas

### Action reversibility in AI planning (new to the table; narrows the claim)

Eiter, Erdem and Faber, "Undoing the effects of action sequences", Journal
of Applied Logic 6(3), 2008, introduce reverse plans: whether the effects
of an action sequence can be undone by another sequence, decided over a
planning domain. Morak, Chrpa, Faber and Fišer, "On the Reversibility of
Actions in Planning", KR 2020, generalize this (uniform reversibility, a
reverse plan that works from every state) and show the decision is at least
as hard as planning, PSPACE-hard unrestricted. Med, Chrpa, Morak and Faber
extend it to non-deterministic actions (ICAPS 2024; KR 2025), and ASP
encodings exist (Faber et al., 2021 onward).

What was checked: the KR 2020 page and abstract; the 2008 paper's abstract;
the 2024 and 2025 follow-ups' titles and abstracts.

Delta: this is a compile-time decision about undoability, so the abstract
idea predates rue. It decides by search over a STRIPS-like world model in
which every action's effects are fully known, and its answer is a reverse
plan or its absence. Rue does not search: it computes from declarations
(footprint kinds, undo body and locus, refusal mode, backstop and its arming
order) and its verdict states where the undo runs, past which step it
cannot, what the step of no return costs, who must acknowledge it, and how
long the plan is bounded. The planning work has no notion of locus, cost,
gate, wane or reach. The claim narrows to "for operations against hosts,
from declared footprints, with locus and cost in the verdict".

### Compensation calculi (new to the table; the claim survives)

Bruni, Melgratti and Montanari, "Theoretical foundations for compensations
in flow composition languages", POPL 2005; the Sagas calculi and
compensating CSP (Butler, Hoare, Ferreira); Lanese et al. on the expressive
power of compensation primitives; work on static versus dynamic
compensations, where termination is decidable with static compensations and
not with dynamic ones.

What was checked: the POPL 2005 abstract; the survey chapters' abstracts;
the static-versus-dynamic decidability result.

Delta: these give semantics to compensation and prove properties of the
calculi (expressiveness, decidability). None types the footprint an
activity touches or decides, for a given program, whether its compensations
compose; the "static compensation" of the calculi means the compensation is
fixed at installation, not that anything is checked. Rue is the checker
these calculi lack, and its footprint algebra is what makes the check
possible.

### BPEL compensation handlers, model-checked (new to the table; survives)

Formal semantics for WS-BPEL fault, compensation and termination handlers,
and model checking of processes against them (several papers, 2006 onward;
a BIP-based compositional semantics).

Delta: model checking of a given process against properties someone wrote
down, after translation. Not a language-level verdict, no footprints, no
locus. Adjacent in spirit to rue's E-codes, different in kind.

### Sagas and compensating transactions (in the table; survives)

Garcia-Molina and Salem, 1987, and the microservice saga pattern as
Temporal, Cadence and others implement it: local transactions with
hand-written compensations run in reverse order on failure, compensations
required to be idempotent.

What was checked: Temporal's saga documentation and blog posts, including
a documented failure where a compensation ran before the step it undoes had
taken effect.

Delta unchanged: nothing about a compensation is checked before it runs;
there is no undo that outlives the engine, no point of no return, no
locus. The documented compensation-before-effect failure is exactly the
class rue's LIFO interference query and step numbering exist to refuse.

### `commit confirmed` (in the table; survives)

Junos `commit confirmed` (1 to 65535 minutes, default 10), IOS-XR
`commit confirmed`, IOS-XE `configure terminal revert timer`.

Delta unchanged: one device, one kind of change, one trigger. Rue's T3 is
this generalized to any op with a target-standalone undo, and the reach rule
proves the arming order the devices get for free by being the thing they
change.

### Database migrations (in the table; survives, with a precision)

Rails `ActiveRecord::Migration`: `change` methods are reversed by a
`CommandRecorder` that knows which commands have inverses;
`IrreversibleMigration` is raised when a migration is moving down. Flyway
and Liquibase undo scripts are written by hand.

Precision: Rails does keep a fixed list of reversible commands, which is a
static notion. But the check happens at rollback time, not at migration
time, and nothing is said about two migrations' footprints. Delta stands.

### Infrastructure "atomic transaction" frameworks and patents (new; survives)

US 8935570 B2, "Automating infrastructure workflows as atomic transactions"
(filed 2012, granted 2015; SunGard Availability Services, now 11:11
Systems): flows with paired `do()` and `undo()` methods, `undo()` formulated
at runtime from a captured `CurrentState()`. US 10565536 and US 11087258,
"Automated process reversal". Recorded here for awareness; no reading of
their claims is offered.

Delta: runtime pairing of forward and reverse transactions with state
capture; no static verification of the undo is claimed. Rue's verdict is
computed before anything runs.

### Reversible DSLs outside operations (new; survives)

RASQ, a DSL for reversible robot assembly sequences with formal semantics
and reverse execution to back out of an error; Eel, a language for partially
reversible programs with logged trace information for the non-invertible
parts; Ψ-Lisp and the reversible-computing lineage.

Delta: reverse execution at runtime, of programs, over state the language
owns. No footprints on a world the language does not own, no locus, no
cost.

### Reversible programming languages (in the table; survives)

Janus (1982; Yokoyama et al., 2008), RFun (2012), and the Reversible
Computation conference series through 2024.

Delta unchanged: language-level reversibility of computation, not of
effects on a world; the totality ethic is borrowed, the subject is not.

### Agent workflow verification and transactional tool use (new; survives)

Agentproof (Xavier et al., arXiv 2026): static structural checks and
temporal safety policies, compiled to automata, over workflow graphs
extracted from agent frameworks. Atomix (Mohammadi et al., arXiv 2026):
progress-aware transactions for agent tool use, with a commit gate that
lets irreversible effects out only at commit and compensates reversible
ones on abort. Squidie, an Elixir workflow runtime with steps marked
`:irreversible`.

Delta: Agentproof verifies reachability and temporal safety, not
reversibility, footprints or locus. Atomix's commit gate is rue's `knell`
and `commit()` discipline at runtime, with no verdict beforehand. Squidie
marks a step irreversible and does nothing with the mark statically. The
vocabulary is converging on rue's from a different direction, which is
evidence the problem is real and the verdict is the missing piece.

### Ansible, Terraform, Kubernetes, NixOS (in the table; survive)

Ansible has no rollback beyond hand-written playbooks and roles such as
ansistrano.rollback. Terraform and Kubernetes converge; `rollout undo`
redeploys a prior revision. NixOS generations roll back one footprint kind.

Delta unchanged.

### Miniscript (in the table; survives)

Policy satisfiability, minimum-satisfaction analysis and refusal of mixed
timelocks at compile time.

Delta unchanged: policy, not operations. It is the model rue's gates borrow,
including the refusal of a policy the compiler cannot reason about (E0508,
E0509).

### Lenses and bidirectional transformations; Ecto.Multi; Metafont and Dhall

Unchanged from the table. Checked by search for anything newer; nothing
that changes the delta.

## Proposed rewording of the claim

"Rue is the first language for operations against hosts in which 'this can
be undone' is a compile-time verdict rather than a comment: computed from
declared footprints, undo loci and refusal modes rather than by search over
a world model, and stating where the undo runs, past which step it cannot,
what that step costs and who must acknowledge it, and how long the plan is
bounded. Deciding undoability offline is not new (action reversibility in
planning; compensation calculi); deciding it for a plan language from
declarations, with locus and cost in the answer, is."

## Sources

- Eiter, Erdem, Faber, "Undoing the effects of action sequences", J. Applied Logic 6(3), 2008. https://www.sciencedirect.com/science/article/pii/S1570868307000328
- Morak, Chrpa, Faber, Fišer, "On the Reversibility of Actions in Planning", KR 2020. https://proceedings.kr.org/2020/65/
- Chrpa et al., "Universal and Uniform Action Reversibility", KR 2021. https://proceedings.kr.org/2021/63/kr2021-0063-chrpa-et-al.pdf
- Med, Chrpa, Morak, Faber, "Weak and Strong Reversibility of Non-deterministic Actions", ICAPS 2024. https://ojs.aaai.org/index.php/ICAPS/article/view/31496
- Med et al., "Non-deterministic Action Reversibility: Complexity Results", KR 2025. https://proceedings.kr.org/2025/45/kr2025-0045-med-et-al.pdf
- Bruni, Melgratti, Montanari, "Theoretical foundations for compensations in flow composition languages", POPL 2005. https://dl.acm.org/doi/10.1145/1040305.1040323
- "A Process Calculus Analysis of Compensations". https://link.springer.com/chapter/10.1007/978-3-642-00945-7_6
- "On the Expressive Power of Primitives for Compensation Handling". https://link.springer.com/chapter/10.1007/978-3-642-11957-6_20
- "Formal analysis of BPEL workflows with compensation by model checking". https://www.researchgate.net/publication/228703533_Formal_analysis_of_BPEL_workflows_with_compensation_by_model_checking
- Temporal, "Saga Pattern". https://docs.temporal.io/design-patterns/saga-pattern
- Temporal, "Saga Compensating Transactions". https://temporal.io/blog/compensating-actions-part-of-a-complete-breakfast-with-sagas
- Rails API, `ActiveRecord::IrreversibleMigration`. https://api.rubyonrails.org/classes/ActiveRecord/IrreversibleMigration.html
- Junos `commit confirmed` example. https://www.networkcuriosity.com/junos-commit-confirmed-example/
- "How Cisco (IOS/IOS XE) Implements Juniper like Commit and Rollback Behavior". https://iosxrjunos.wordpress.com/2025/05/16/how-cisco-ios-ios-xe-implements-juniper-like-commit-and-rollback-behavior/
- US 8935570 B2, "Automating infrastructure workflows as atomic transactions". https://patents.google.com/patent/US8935570B2/en
- US 10565536, US 11087258, "Automated process reversal". https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/10565536
- "Towards a Domain-Specific Language for Reversible Assembly Sequences" (RASQ). https://link.springer.com/chapter/10.1007/978-3-319-20860-2_7
- "Toward an Energy Efficient Language and Compiler for (Partially) Reversible Algorithms" (Eel). https://arxiv.org/pdf/1605.08475
- Yokoyama et al., "Principles of a reversible programming language" (Janus). https://dl.acm.org/doi/10.1145/1366230.1366239
- "Interpretation and programming of the reversible functional language RFUN". https://dl.acm.org/doi/10.1145/2897336.2897345
- Xavier et al., "Agentproof: Static Verification of Agent Workflow Graphs", 2026. https://arxiv.org/abs/2603.20356
- Mohammadi et al., "Atomix: Timely, Transactional Tool Use for Reliable Agentic Workflows", 2026. https://arxiv.org/abs/2602.14849
- Squidie, workflow automation runtime for Elixir. https://elixirforum.com/t/squidie-workflow-automation-runtime-for-elixir-applications/75162
- ansistrano/rollback. https://github.com/ansistrano/rollback
- rust-miniscript, mixed timelock detection. https://github.com/rust-bitcoin/rust-miniscript/pull/121
- Blockstream, "Don't Mix Your Timelocks". https://medium.com/blockstream/dont-mix-your-timelocks-d9939b665094

---

# The name sweep

ROADMAP section 12 requires a search of crates.io, PyPI, npm and GitHub for
`rue`, `rued` and `rue-core`, and for the SDKs' publishing names, recorded
here **before anything is public**; it is one of Phase 5's exit criteria.
Swept 2026-09-12.

## Verdict

**The name `rue` is taken for a programming language, twice, and the two
package names rue would publish first are held by one of them.** Nothing is
blocked today -- rue publishes nothing and its repository is private -- and
renaming is the owner's decision and nobody else's. What the sweep can say
is what a public rue would walk into.

## What is taken

| Registry | Name | Held by | Evidence |
|---|---|---|---|
| crates.io | `rue` | "The Rue programming language", 0.1.0, 2025-12-21, 35 downloads | `github.com/xch-dev/rue`, homepage `rue-lang.com` — a typed language for Chia targeting CLVM bytecode |
| crates.io | `rue-lsp` | the same project's language server, 0.10.0, 2026-07-26, 4202 downloads | `github.com/xch-dev/rue` |
| crates.io | `rue-core` | "A Vue 3-like reactive UI framework", 0.1.0, 2026-05-15, 21 downloads | unrelated |
| PyPI | `rue` | "Testing Framework for AI Software", 0.1.0 | unrelated |
| npm | `rue` | "nodejs dependency injection container", 0.9.2 | unrelated |
| GitHub | `rue-language/rue` | a second language called Rue, 1193 stars | "higher level than Rust but lower level than…" |
| GitHub | `xch-dev/rue` | the crates.io holder, 47 stars | as above |
| GitHub | `fasterthanlime/rue` | "a bad version of strace in Rust", 51 stars | unrelated |

## What is free

`rued` (crates.io, PyPI, npm); `rue-hook`, `rue-hook-sdk` (crates.io);
`rue-hook`, `rue_hook` (PyPI); `rue-hook`, `tree-sitter-rue` (npm);
`dev.rue` (Maven Central, no group); `Rue` and `Rue.Hook` (NuGet); `rue`
and `rue_hook` (Hex).

So every name the SDKs need is available under every ecosystem's own
convention. What is not available is the one name the project is called.

## What this does and does not mean

It is not a legal question — no trademark search was done, and none is
being claimed here. It is a collision question, and it has two halves.

The **discovery** half: someone searching "rue language" today finds two
other languages, one with a domain of its own. A third would be hard to
find and easy to confuse, and every answer to "does rue do X?" would have to
begin by asking which rue.

The **publishing** half is narrower than it looks. A private repository and
a tarball need no registry at all. What a registry name is needed for is
`cargo install rue`, and that name is gone; `rued` is free, and a project
whose daemon is `rued` could publish the pair as `rued` and `rued-cli`
without touching `rue` — the binary an operator types would still be `rue`.

## The owner's options, for the record

1. **Keep the name, publish under `rued`.** Nothing in the repository
   changes. The collision stays a discovery problem, and the README's first
   line has to disambiguate.
2. **Keep the name privately, decide at publication.** The sweep is done;
   the decision waits for the decision to go public, which is the owner's
   anyway.
3. **Rename.** The cost is not the code — the seam guard would find every
   occurrence in a morning — it is every document, every tenant, the
   protocol's own field names (`rue_root`, `rue-region`, `# rue-region`
   markers written into strangers' files, `dev.rue`), and the store's
   on-disk paths. That cost rises with every release, and it is lowest now.

Recorded, not decided.

## Sources

- crates.io: `https://crates.io/api/v1/crates/{rue,rued,rue-core,rue-hook,rue-hook-sdk,rue-lsp}`
- PyPI: `https://pypi.org/pypi/{rue,rued,rue-hook,rue_hook,rue-core}/json`
- npm: `https://registry.npmjs.org/{rue,rued,rue-hook,tree-sitter-rue}`
- GitHub: `https://api.github.com/search/repositories?q=rue+in:name&sort=stars`
- Maven Central: `https://search.maven.org/solrsearch/select?q=g:dev.rue`
- NuGet: `https://azuresearch-usnc.nuget.org/query?q=packageid:{Rue,Rue.Hook}`
- Hex: `https://hex.pm/api/packages/{rue,rue_hook}`
