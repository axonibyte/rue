//! `rue fmt` (docs/ROADMAP.md 6.9): the canonical layout, which is the
//! tenant corpus's own. Two spaces per block depth; one space after a
//! comma and after a keyword's colon; none inside brackets or around a
//! dot; a call's parenthesis touches its name; binary operators spaced;
//! a trailing comment three spaces after the statement; blank lines and
//! comments kept as written; lines never rewrapped. Idempotent, and the
//! identity on every tenant file.

use crate::syntax::SyntaxKind::{self, *};
use crate::syntax::{SyntaxElement, SyntaxNode};

/// Format a parsed tree. The tree must have no `ERROR` nodes; a caller
/// formats only what parsed clean, so no text is lost in a rewrite.
pub fn format(root: &SyntaxNode) -> String {
    let mut out = String::new();
    let mut line: Vec<(SyntaxKind, String, Option<SyntaxKind>, bool)> = Vec::new();
    let mut depth: usize = 0;
    let mut comment: Option<String> = None;
    let flush = |out: &mut String,
                 line: &mut Vec<(SyntaxKind, String, Option<SyntaxKind>, bool)>,
                 comment: &mut Option<String>,
                 depth: &mut usize| {
        let first = line.first().map(|(k, t, _, _)| (*k, t.clone()));
        let last = line.last().map(|(k, t, _, _)| (*k, t.clone()));
        let closes = matches!(&first, Some((NAME, t)) if t == "end" || t == "else");
        let opens = matches!(&last, Some((NAME, t)) if t == "do");
        let indent_depth = if closes {
            depth.saturating_sub(1)
        } else {
            *depth
        };
        if !line.is_empty() {
            out.push_str(&"  ".repeat(indent_depth));
            let mut prev: Option<(SyntaxKind, Option<SyntaxKind>)> = None;
            for (kind, text, parent, unary_minus) in line.iter() {
                if let Some((pk, pparent)) = prev {
                    if space_between(pk, pparent, *kind, *parent) {
                        out.push(' ');
                    }
                }
                out.push_str(text);
                prev = Some((*kind, if *unary_minus { Some(UNARY) } else { *parent }));
            }
            if let Some(c) = comment.take() {
                out.push_str("   ");
                out.push_str(&c);
            }
        } else if let Some(c) = comment.take() {
            out.push_str(&"  ".repeat(*depth));
            out.push_str(&c);
        }
        out.push('\n');
        if closes && matches!(&first, Some((NAME, t)) if t == "end") {
            *depth = depth.saturating_sub(1);
        }
        if opens {
            *depth += 1;
        }
        // `else` closes the then-arm and opens the else-arm: same depth.
        line.clear();
    };
    for el in root.descendants_with_tokens() {
        let SyntaxElement::Token(tok) = el else {
            continue;
        };
        match tok.kind() {
            WHITESPACE => {}
            COMMENT => comment = Some(tok.text().to_string()),
            NEWLINE => flush(&mut out, &mut line, &mut comment, &mut depth),
            k => {
                let parent = tok.parent().map(|p| p.kind());
                let unary_minus = k == MINUS && parent == Some(UNARY);
                line.push((k, tok.text().to_string(), parent, unary_minus));
            }
        }
    }
    if !line.is_empty() || comment.is_some() {
        flush(&mut out, &mut line, &mut comment, &mut depth);
    }
    out
}

/// Whether one space separates two adjacent tokens on a line.
fn space_between(
    prev: SyntaxKind,
    prev_parent: Option<SyntaxKind>,
    next: SyntaxKind,
    next_parent: Option<SyntaxKind>,
) -> bool {
    match next {
        COMMA | R_PAREN | R_BRACK | R_BRACE | DOT => return false,
        COLON => return false,
        L_PAREN if next_parent == Some(CALL) => return false,
        _ => {}
    }
    match prev {
        L_PAREN | L_BRACK | L_BRACE | PERCENT_BRACE | DOT => false,
        MINUS if prev_parent == Some(UNARY) => false,
        _ => true,
    }
}
