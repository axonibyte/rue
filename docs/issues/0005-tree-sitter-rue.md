# 0005: tree-sitter-rue: a grammar for editors

- status: open
- kind: feature
- phase: 5
- opened: 2026-09-11

A tree-sitter grammar for highlighting in editors (ROADMAP Phase 5;
acceptance: "editor highlighting ... on all tenant files").

**Where.** `tree-sitter-rue/` at the repository root: `grammar.js`, the
generated parser checked in, highlight queries. Editors fetch a grammar
from a subdirectory, and the GitHub mirror is the public URL.

**Done when.** Every tenant text and parser-corpus snippet parses with no
error node; a guard compares its parse with the Rust parser's over the
same files, so the two grammars cannot drift; regenerating the parser runs
where Node and the tree-sitter CLI are (the pipeline and the Ubuntu guest),
not in the local gate, and a check requires the checked-in parser to match
`grammar.js`.
