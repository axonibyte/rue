# 0006: A language server: diagnostics and hover

- status: closed
- kind: feature
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-12

An LSP over the existing front end and checker: diagnostics as a text is
edited, and hover on a step showing its footprint, undo, locus and drift
policy (ROADMAP Phase 5; acceptance: hover on all tenant files).

**Before building.** The roadmap names `tower-lsp`; its release cadence and
its maintained fork are to be compared first, and its dependency tree
checked against `tools/darwin-denylist.txt`, since the darwin binaries are
cross-built with no SDK. A new workspace crate (`lsp/`, `rue-lsp`).

**The comparison the issue asked for (2026-09-12).**

| crate | newest | last release | downloads | what it is |
|---|---|---|---|---|
| `tower-lsp` 0.20.0 | 0.20.0 | 2023-08-11 | 7.5M | what ROADMAP 1277 named; three years without a release |
| `tower-lsp-f` 0.26.0 | 0.26.0 | 2026-07-04 | 25k | the maintained fork; small user base |
| `async-lsp` 0.2.4 | 0.2.4 | 2026-04-24 | 1.4M | maintained, tower-based, wants an async runtime |
| `lsp-server` 0.10.0 | 0.10.0 | 2026-07-16 | 15.5M | rust-analyzer's transport: framing and JSON-RPC, nothing else |

**Built on `lsp-server` + `lsp-types`, not `tower-lsp`.** Three reasons, in
order. The crate the roadmap named has not been released since August 2023
and its users have moved to a fork or to `async-lsp`. rue's workspace has no
async runtime at all -- `grep tokio Cargo.lock` finds nothing -- and a
language server is the worst possible reason to acquire one: judging a rue
text takes a millisecond, and an editor that asks twice gets two answers in
order from a plain loop. And the dependency: `rue-lsp` pulls exactly
`lsp-server`, `lsp-types`, `serde` and `serde_json` beside rue's own crates,
and `tools/lint-darwin-deps.sh` stays clean at 102 crates with none denied,
which is the check this issue asked for before anything was written.

ROADMAP 1277's `tower-lsp` was a lean, not a mandate; this is the pushback
section 0.3 asks for, with the evidence.

**Closed.** Built, 2026-09-12, on `lsp-server` + `lsp-types` rather than the `tower-lsp` the roadmap named; the comparison is in the table above and the short of it is that tower-lsp has had no release since August 2023, the workspace has no async runtime, and judging a rue text takes a millisecond. `rue-lsp` pulls four crates beside rue's own, and `tools/lint-darwin-deps.sh` stays clean.

`lsp/` serves diagnostics as a text is edited and hover on a step; the diagnostics are the front end's and the checker's own, and the hover is what `explain` prints -- an editor that invented a second opinion about a plan would teach an operator which of the two to believe, and it would not always be the right one. The capabilities promise those two things and nothing else.

Writing the tests decided the design. A file whose clauses dispatch on the host cannot be resolved without one (E0112), which is an argument `rue check` is given and an editor is not: the first draft would have underlined every clause-dispatched tenant in the project. The server now names a host itself -- the first its own inventory lists -- and says so in every diagnostic and hover it reports. An unsaved buffer is parsed and not checked, because a resolve reads imports and the inventory from disk and would underline a line the author has already fixed. Positions are counted in UTF-16 code units, pinned by a test with an em dash in a comment.

The Ubuntu guest then found what the workstation could not: the test helper built a document URI by pasting a path after `file://`, which is what nothing does -- an editor percent-encodes. The library gained `uri_of` beside `path_of` and a round trip over a path with a space, a non-ASCII character, a plus and a percent sign. Row `lsp-underlines-a-missing-host`. Not proven: any editor actually driving it; the tests call the handlers, not a process.
