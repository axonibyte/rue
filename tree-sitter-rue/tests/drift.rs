//! The guard that keeps two parsers from drifting apart (docs/issues/0005).
//!
//! rue has two parsers now: the Rust front end, which decides what a text
//! means, and this grammar, which decides what an editor shows. They read
//! the same files, so they must agree about which files are rue -- and the
//! only way to know they do is to run both over every text the repository
//! holds and compare the answers.
//!
//! The comparison is deliberately coarse: accepted or refused. A tree-sitter
//! parse tree and a rowan tree are different shapes and always will be, and
//! a guard that demanded the same tree would be a guard nobody could keep.
//! What it catches is the thing that matters to an operator: a text rue
//! accepts and the editor underlines, or a text rue refuses and the editor
//! shows as clean.
//!
//! One diagnostic is outside the comparison and named here rather than
//! quietly dropped. E0105 judges the version marker -- whether *this*
//! compiler reads the text -- and a grammar that refused `rue 7` would
//! leave every editor blind on the day rue's version turns over, while one
//! that accepted a file with no marker at all would call something rue that
//! is not. So the grammar requires the marker and does not read its number,
//! and a text whose only complaint is E0105 is not asked about here.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the repository root")
        .to_path_buf()
}

/// Every `.rue` text under the repository, tenants and corpus alike.
fn every_rue_text(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in ["tenants", "surface/tests/corpus"] {
        walk(&root.join(dir), &mut out);
    }
    out.sort();
    assert!(
        out.len() > 50,
        "the guard found only {} texts; it is checking nothing",
        out.len()
    );
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "rue") {
            out.push(p);
        }
    }
}

fn parser() -> tree_sitter::Parser {
    let mut p = tree_sitter::Parser::new();
    p.set_language(&tree_sitter_rue::LANGUAGE.into())
        .expect("the grammar loads");
    p
}

/// Whether a tree holds an error node anywhere, which is what an editor
/// paints as a syntax error.
fn has_error(node: tree_sitter::Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    let any = node.children(&mut cursor).any(has_error);
    any
}

#[test]
fn the_grammar_accepts_every_text_the_front_end_accepts() {
    let root = repo_root();
    let mut p = parser();
    let mut checked = 0;
    for path in every_rue_text(&root) {
        let src = std::fs::read_to_string(&path).expect("the text");
        let rue = rue_surface::parse(&src, &path.to_string_lossy());
        let tree = p.parse(&src, None).expect("a tree");
        let ours = !has_error(tree.root_node());
        let rel = path.strip_prefix(&root).unwrap_or(&path).display();
        let version_only = !rue.diagnostics.is_empty()
            && rue
                .diagnostics
                .iter()
                .all(|d| d.code == rue_core::diagnostics::Code::E0105);
        if version_only {
            continue;
        }
        assert_eq!(
            rue.is_clean(),
            ours,
            "{rel}: the front end {} it and the grammar {} it",
            if rue.is_clean() { "accepts" } else { "refuses" },
            if ours { "accepts" } else { "refuses" },
        );
        checked += 1;
    }
    assert!(checked > 50, "only {checked} texts were compared");
}

/// The queries load against the grammar: a highlight query naming a node
/// this grammar does not have is a file every editor reports and no test
/// would otherwise catch.
#[test]
fn the_highlight_queries_load() {
    let language: tree_sitter::Language = tree_sitter_rue::LANGUAGE.into();
    tree_sitter::Query::new(&language, tree_sitter_rue::HIGHLIGHTS_QUERY)
        .unwrap_or_else(|e| panic!("queries/highlights.scm: {e}"));
}

/// The checked-in parser is the one `grammar.js` describes. Regenerating
/// needs Node and the tree-sitter CLI, which the gate does not require, so
/// what the gate can check is that nobody edited one without the other:
/// `src/grammar.json` is the CLI's own rendering of `grammar.js`, and its
/// rule names and the parser's must agree.
#[test]
fn the_checked_in_parser_matches_the_grammar() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(dir.join("src/grammar.json")).expect("src/grammar.json");
    let js = std::fs::read_to_string(dir.join("grammar.js")).expect("grammar.js");
    let grammar: serde_json::Value = serde_json::from_str(&json).expect("the generated grammar");
    let rules = grammar
        .get("rules")
        .and_then(|r| r.as_object())
        .expect("its rules");
    assert!(rules.len() > 40, "only {} rules", rules.len());
    for name in rules.keys() {
        assert!(
            js.contains(&format!("{name}:")),
            "the generated parser has a rule `{name}` that grammar.js does not: \
             regenerate with `tree-sitter generate`"
        );
    }
    // And every rule the grammar file declares reached the parser.
    let language: tree_sitter::Language = tree_sitter_rue::LANGUAGE.into();
    assert!(
        language.node_kind_count() > rules.len(),
        "the compiled parser knows fewer kinds than the grammar has rules"
    );
}
