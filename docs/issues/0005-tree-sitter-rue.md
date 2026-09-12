# 0005: tree-sitter-rue: a grammar for editors

- status: closed
- kind: feature
- phase: 5
- opened: 2026-09-11
- closed: 2026-09-12

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

**Closed.** Done, 2026-09-12. `tree-sitter-rue/` holds `grammar.js`, the parser generated from it and checked in, `queries/highlights.scm`, a `package.json` for npm, a Cargo crate so the guard can load it, and a README with the Neovim and Helix stanzas. It parses every tenant text and every corpus snippet the Rust front end accepts, with no error node, and refuses every text the front end refuses with a parse error.

The guard (`tree-sitter-rue/tests/drift.rs`, an ordinary workspace test needing only a C compiler) runs both parsers over every `.rue` text under `tenants/` and `surface/tests/corpus/` and requires them to agree on which files are rue; it also loads the highlight queries against the grammar, so a query naming a node the grammar lacks is caught here rather than by every editor in turn, and checks that the checked-in parser and `grammar.js` describe the same rules -- which is what the gate can say without Node.

Three decisions worth recording. E0105 is outside the comparison and named in the test: the grammar requires the version marker and does not read its number, because a grammar that refused `rue 7` would blind every editor the day the version turns over. `repeat over: ... as g` with no `max:` parses here, because the front end parses it and the checker refuses it (E0106) -- a grammar stricter than the parser would have an editor underline what rue reports as a diagnostic. And the contextual-keyword rule of 6.2 shapes the lexer: `owned:`, `os:` and every other name-plus-colon is one token, so the footprint kinds and the pattern keys are written that way.

Regeneration needs Node and the tree-sitter CLI, which the gate does not require. npm on the workstation is broken (its own vendored tree is missing `imurmurhash`); `cargo install tree-sitter-cli` builds the same CLI from crates.io and uses the `node` binary, which works. Row `grammar-edited-without-regenerating`. Not proven: highlighting as an editor actually paints it.
