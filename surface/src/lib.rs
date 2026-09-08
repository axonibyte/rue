//! rue-surface: the `.rue` front end (docs/ROADMAP.md section 6). Unit A
//! of Phase 2: the lexer, the parser with recovery over a lossless rowan
//! tree, and the formatter. The resolver to `rue_core::ir::PlanIr` is unit
//! B. Depends on core only; no I/O.

pub mod ast;
pub mod fmt;
pub mod lexer;
pub mod parser;
pub mod resolve;
pub mod syntax;

use rue_core::diagnostics::Diagnostic;
use syntax::{SyntaxKind, SyntaxNode};

pub use parser::LANGUAGE_VERSION;

/// A parsed file: the lossless tree and the diagnostics parsing raised.
pub struct Parse {
    pub root: SyntaxNode,
    pub diagnostics: Vec<Diagnostic>,
}

impl Parse {
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Parse a source text. `file` names it in diagnostics.
pub fn parse(src: &str, file: &str) -> Parse {
    let p = parser::parse(src, file);
    Parse {
        root: SyntaxNode::new_root(p.green),
        diagnostics: p.diagnostics,
    }
}

/// Format a source text, or refuse with the parse diagnostics: a tree
/// with errors is not rewritten, so no text can be lost.
pub fn format(src: &str, file: &str) -> Result<String, Vec<Diagnostic>> {
    let p = parse(src, file);
    if !p.is_clean() {
        return Err(p.diagnostics);
    }
    Ok(fmt::format(&p.root))
}

/// The tree as text, one node or token per line, indented by depth:
/// `KIND@start..end` for nodes, `KIND@start..end "text"` for tokens.
/// The parser goldens hold this.
pub fn dump(root: &SyntaxNode) -> String {
    fn go(el: syntax::SyntaxElement, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        match el {
            syntax::SyntaxElement::Node(n) => {
                out.push_str(&format!("{pad}{:?}@{:?}\n", n.kind(), n.text_range()));
                for c in n.children_with_tokens() {
                    go(c, depth + 1, out);
                }
            }
            syntax::SyntaxElement::Token(t) => {
                if t.kind() == SyntaxKind::WHITESPACE {
                    return;
                }
                out.push_str(&format!(
                    "{pad}{:?}@{:?} {:?}\n",
                    t.kind(),
                    t.text_range(),
                    t.text()
                ));
            }
        }
    }
    let mut out = String::new();
    go(syntax::SyntaxElement::Node(root.clone()), 0, &mut out);
    out
}
