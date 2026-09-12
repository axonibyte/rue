//! rue's tree-sitter grammar, as a language an editor or a test can load.
//!
//! The grammar is `grammar.js`; `src/parser.c` is generated from it and
//! checked in, because editors fetch a grammar and expect the parser to be
//! there, and because the gate must be able to run the drift guard on a
//! machine with no Node and no tree-sitter CLI (`docs/issues/0005`).

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_rue() -> *const ();
}

/// The language this grammar defines.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_rue) };

/// The highlight queries, for an editor that wants them from the crate.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
