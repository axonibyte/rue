# 0003: Drill mode: scheduled apply-and-recant on a canary, with an attestation

- status: open
- kind: feature
- phase: 5
- opened: 2026-09-11

A drill applies a plan to a designated canary host on a schedule,
recants it, and journals an attestation that the undo restored the canary,
verifiable against the journal chain (ROADMAP Phase 5; acceptance: "drill
attestation journaled and verified").

**Design first.** How a drill is declared (a plan option or a definition
of its own), what schedules it (`rued` or an outside timer), what the
attestation holds (each observed fact's digest before apply and after
recant, bound to the chain) and how `rue journal verify` checks it. A
one-page design goes to the owner before any code.
