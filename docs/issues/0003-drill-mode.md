# 0003: Drill mode: scheduled apply-and-recant on a canary, with an attestation

- status: closed
- kind: feature
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-12

A drill applies a plan to a designated canary host on a schedule,
recants it, and journals an attestation that the undo restored the canary,
verifiable against the journal chain (ROADMAP Phase 5; acceptance: "drill
attestation journaled and verified").

**Design first.** How a drill is declared (a plan option or a definition
of its own), what schedules it (`rued` or an outside timer), what the
attestation holds (each observed fact's digest before apply and after
recant, bound to the chain) and how `rue journal verify` checks it. A
one-page design goes to the owner before any code.

**Design (2026-09-12).** Written here rather than sent, because the owner
left the open questions to my judgment; every decision below is reversible
by an objection on this issue.

*A verb, not grammar.* `rue drill <file> --host <canary> [--plan P]`. No new
keyword, so the grammar stays settled while the editor tooling is being
built on it (ROADMAP 11), and nothing in tree-sitter, the LSP, the parser
corpus or the Haskell prototype moves for this.

*What a canary is.* A host whose inventory record carries the role
`canary`. Any other host refuses before anything is applied, R0410. A drill
is a real apply of a real plan on a real host, and that refusal is the only
thing between it and production; it is not overridable by a flag.

*What schedules it.* The operator's own timer -- cron, a systemd timer,
launchd -- invoking `rue drill`, with the line to use in the docs. `rued`
grows no scheduler of its own: rue schedules *backstops* through the site's
scheduler binding because they must fire with the engine dead, and a drill
is the opposite case -- it needs the engine alive -- so a timer that runs
the verb is both smaller and honest about who is doing the scheduling.

*What it does.* Reads the digest of every fact the plan's footprint names on
the canary; applies; recants; reads them again; compares. The plan is an
ordinary instance throughout -- it reserves and releases in the ledger like
any other, so a drill contends with production work rather than bypassing
it.

*What the attestation holds and where it lives.* One journal entry,
`DrillAttested{plan, host, instance, restored, facts}`, each fact one line
`<shape> before=<digest|unknown> after=<digest|unknown>`. It is a journal
entry and nothing else, so the chain covers it and `rue journal verify`
already proves it was not edited after the fact -- an attestation with a
verification of its own would be a second chain to trust. `rue journal
verify --attestations` prints each one and its verdict once the chain
verifies.

*Facts that cannot be read.* A shape whose executor cannot read it (R0205's
case) is `unknown` on both sides and never counts as restored: a drill with
any unknown fact does not attest, and says which shapes it could not read.
Attesting over facts nobody read is exactly the thing the drill exists to
disprove.

*Exit codes.* 0 the canary was restored; 1 it was not, or a fact could not
be read, with the shapes named; 3 the plan refused or could not be applied;
2 the host is not a canary, or the usage is wrong.

**Rows:** `drill-on-a-non-canary`, `drill-attests-an-unread-fact`.

**Closed.** Built as designed above, 2026-09-12, and proven on both reaper guests. `rue drill <file> --host <canary>` reads the digest of every fact the plan's footprint names on the hosts it touches, applies, recants, reads them again and journals `DrillAttested{plan, host, instance, restored, facts}`; `rue journal verify --attestations` prints each one once the chain verifies, and exits non-zero on a chain carrying no drill or one that attested nothing. Both refusals are R0410: a host without the role `canary`, a step whose host is bound at runtime, and a permanent plan, which cannot be recanted. A fact the engine could not read stays `unread` and attests to nothing, as does a footprint that named no readable fact. The design's one unstated decision, settled while building: the drill also refuses on a dry-run daemon, because a rehearsal of an apply-and-recant proves nothing about either. `tenants/e2e/tests/drill.rs` runs the whole of it against a real canary over a real sshd on both guests, and reads the attestation back out of the verified chain, which is the acceptance line's 'journaled and verified'. Rows `drill-on-a-non-canary` and `drill-attests-an-unread-fact` verified.
