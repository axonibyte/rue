# 0006: A language server: diagnostics and hover

- status: open
- kind: feature
- phase: 5
- opened: 2026-09-11

An LSP over the existing front end and checker: diagnostics as a text is
edited, and hover on a step showing its footprint, undo, locus and drift
policy (ROADMAP Phase 5; acceptance: hover on all tenant files).

**Before building.** The roadmap names `tower-lsp`; its release cadence and
its maintained fork are to be compared first, and its dependency tree
checked against `tools/darwin-denylist.txt`, since the darwin binaries are
cross-built with no SDK. A new workspace crate (`lsp/`, `rue-lsp`).
