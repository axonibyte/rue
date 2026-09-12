# tree-sitter-rue

rue's tree-sitter grammar: highlighting and navigation for editors
(`docs/ROADMAP.md` section 6 is the language; `docs/issues/0005` is why this
exists).

This grammar decides what an editor *shows*. It does not decide what a text
*means* — `rue check` does, and it is the only authority on that. A file
this grammar parses without complaint can still be refused by the checker,
and should be: the diagnostics are the point of rue, and an editor that
painted a plan green because its shape was right would be lying about the
part that matters.

## Using it

Most editors want the repository and a subdirectory. The grammar lives at
the repository root under `tree-sitter-rue/`, with `queries/highlights.scm`
beside it.

Neovim, with `nvim-treesitter`:

```lua
require('nvim-treesitter.parsers').get_parser_configs().rue = {
  install_info = {
    url = 'https://bitbucket.org/axonibyte/rue',
    location = 'tree-sitter-rue',
    files = { 'src/parser.c' },
    branch = 'main',
  },
  filetype = 'rue',
}
```

Helix, in `languages.toml`:

```toml
[[language]]
name = "rue"
scope = "source.rue"
file-types = ["rue"]
comment-token = "#"
indent = { tab-width = 2, unit = "  " }

[[grammar]]
name = "rue"
source = { git = "https://bitbucket.org/axonibyte/rue", subpath = "tree-sitter-rue", rev = "main" }
```

## Changing it

`grammar.js` is the source; `src/parser.c` is generated from it and checked
in, because editors fetch a grammar and expect a parser to be there, and
because the gate must run the drift guard on a machine with neither Node nor
the tree-sitter CLI.

```
tree-sitter generate      # rewrites src/ from grammar.js
cargo test -p tree-sitter-rue
```

The CLI is a Rust crate as well as an npm package: `cargo install
tree-sitter-cli` works where npm does not, and needs `node` on the path to
evaluate `grammar.js`.

## What the guard holds

`tests/drift.rs` parses every `.rue` text under `tenants/` and
`surface/tests/corpus/` with both this grammar and the Rust front end, and
requires them to agree on which files are rue. A text the front end accepts
must have no error node here; a text it refuses with a parse error must have
one. Two parsers over one language drift apart the moment nobody is
comparing them, and an editor that underlines a valid plan — or stays silent
on an invalid one — teaches an operator to distrust the tool that is right.

One diagnostic sits outside that comparison, and is named in the test rather
than dropped from it: `E0105` judges the version marker, and this grammar
requires the marker without reading its number. A grammar that refused
`rue 7` would blind every editor the day rue's version turns over.
