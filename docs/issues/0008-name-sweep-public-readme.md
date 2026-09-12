# 0008: The name sweep, and a public README

- status: closed
- kind: feature
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-12

Phase 5's exit criteria: search crates.io, PyPI, npm and GitHub for
`rue`, `rued` and `rue-core` -- and the SDKs' names, `rue-hook`,
`rue-hook-sdk`, `rue_hook`, `dev.rue:rue-hook`, `Rue.Hook` -- and record the
result in `docs/prior-art.md` before anything is public (ROADMAP 12); a
public README whose prior-art section is 1.2's.

Going public is the owner's decision; the sweep can be done before it.

**Closed.** The name sweep is done and recorded in `docs/prior-art.md` (2026-09-12), which is what ROADMAP 12 asks for before anything is public. `rue` is taken on crates.io (by another programming language called Rue, `xch-dev/rue`, homepage rue-lang.com), on PyPI (an AI testing framework) and on npm (a DI container); `rue-lsp` is taken by that same language's server and `rue-core` by an unrelated UI framework; GitHub holds a second language called Rue at 1193 stars. Free everywhere checked: `rued`, `rue-hook`, `rue-hook-sdk`, `rue_hook`, `tree-sitter-rue`, `dev.rue`, `Rue.Hook`. The record states the discovery problem, the narrower publishing problem (`cargo install rue` is gone; `rued` is free), and three options, and decides none of them: renaming and publishing are both the owner's.

The public README is not written and is deliberately left: it is the face of a project whose name may change, and ROADMAP 12's own order is that the sweep comes first. Filed as its own issue (`docs/issues/0016`) so Phase 5 can exit with the sweep done and the README named rather than half-written.
