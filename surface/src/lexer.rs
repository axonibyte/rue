//! The lexer (docs/ROADMAP.md 6.2): every token of the surface as a
//! `(SyntaxKind, &str)` pair over the source, nothing dropped, so the
//! tree the parser builds is the source byte for byte. Strings carry their
//! `#{...}` interpolations whole, braces balanced, so a string is one
//! token; the resolver reads the parts.

use logos::Logos;

use crate::syntax::SyntaxKind;

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    #[regex(r"[ \t]+")]
    Whitespace,
    #[regex(r"\r?\n")]
    Newline,
    // A comment is the rest of its line by definition, so the greedy scan is
    // the intended one.
    #[regex(r"#[^\n]*", allow_greedy = true)]
    Comment,
    // A duration before an integer, so `30m` is one token.
    #[regex(r"[0-9]+(ms|s|m|h|d)")]
    Duration,
    #[regex(r"[0-9]+\.[0-9]+")]
    Float,
    #[regex(r"[0-9]+")]
    Int,
    // A trailing `?` belongs to the builtins (`defined?`, `unknown?`,
    // `all_eq?`, `any_eq?`, `member?`), section 6.5.
    #[regex(r"[a-z_][a-z0-9_]*\??")]
    Name,
    #[regex(r"[A-Z][A-Za-z0-9_]*")]
    UpperName,
    #[regex(r":[a-z_][a-z0-9_?!]*")]
    Atom,
    #[regex(r#":"([^"\\\n]|\\.)*""#)]
    QuotedAtom,
    #[token("\"", lex_string)]
    Str,
    #[token("%{")]
    PercentBrace,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBrack,
    #[token("]")]
    RBrack,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("|>")]
    PipeGt,
    #[token("|")]
    Pipe,
    #[token("==")]
    EqEq,
    #[token("!=")]
    BangEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token(".")]
    Dot,
}

/// A string: from the opening quote to the closing one, with `\"` escapes
/// and `#{...}` interpolations whose braces balance. An unterminated
/// string runs to the end of the line and is reported by the parser.
fn lex_string(lex: &mut logos::Lexer<Tok>) -> bool {
    let rest = lex.remainder();
    let bytes = rest.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if depth == 0 => i += 2,
            b'"' if depth == 0 => {
                lex.bump(i + 1);
                return true;
            }
            b'#' if depth == 0 && bytes.get(i + 1) == Some(&b'{') => {
                depth = 1;
                i += 2;
            }
            b'{' if depth > 0 => {
                depth += 1;
                i += 1;
            }
            b'}' if depth > 0 => {
                depth -= 1;
                i += 1;
            }
            b'\n' => break,
            _ => i += 1,
        }
    }
    // Unterminated: take the rest of the line as the token.
    lex.bump(i);
    true
}

/// One token: its kind and its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: SyntaxKind,
    pub text: &'a str,
}

/// Lex the whole source. Every byte lands in exactly one token; a byte the
/// lexer does not know is an `ERROR_TOKEN` of one character.
pub fn lex(src: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut lexer = Tok::lexer(src);
    while let Some(t) = lexer.next() {
        let text = lexer.slice();
        let kind = match t {
            Ok(Tok::Whitespace) => SyntaxKind::WHITESPACE,
            Ok(Tok::Newline) => SyntaxKind::NEWLINE,
            Ok(Tok::Comment) => SyntaxKind::COMMENT,
            Ok(Tok::Duration) => SyntaxKind::DURATION,
            Ok(Tok::Float) => SyntaxKind::FLOAT,
            Ok(Tok::Int) => SyntaxKind::INT,
            Ok(Tok::Name) => SyntaxKind::NAME,
            Ok(Tok::UpperName) => SyntaxKind::UPPER_NAME,
            Ok(Tok::Atom) | Ok(Tok::QuotedAtom) => SyntaxKind::ATOM,
            Ok(Tok::Str) => SyntaxKind::STRING,
            Ok(Tok::PercentBrace) => SyntaxKind::PERCENT_BRACE,
            Ok(Tok::LParen) => SyntaxKind::L_PAREN,
            Ok(Tok::RParen) => SyntaxKind::R_PAREN,
            Ok(Tok::LBrack) => SyntaxKind::L_BRACK,
            Ok(Tok::RBrack) => SyntaxKind::R_BRACK,
            Ok(Tok::LBrace) => SyntaxKind::L_BRACE,
            Ok(Tok::RBrace) => SyntaxKind::R_BRACE,
            Ok(Tok::Comma) => SyntaxKind::COMMA,
            Ok(Tok::Colon) => SyntaxKind::COLON,
            Ok(Tok::PipeGt) => SyntaxKind::PIPE_GT,
            Ok(Tok::Pipe) => SyntaxKind::PIPE,
            Ok(Tok::EqEq) => SyntaxKind::EQ_EQ,
            Ok(Tok::BangEq) => SyntaxKind::BANG_EQ,
            Ok(Tok::Le) => SyntaxKind::LE,
            Ok(Tok::Ge) => SyntaxKind::GE,
            Ok(Tok::Eq) => SyntaxKind::EQ,
            Ok(Tok::Lt) => SyntaxKind::LT,
            Ok(Tok::Gt) => SyntaxKind::GT,
            Ok(Tok::Plus) => SyntaxKind::PLUS,
            Ok(Tok::Minus) => SyntaxKind::MINUS,
            Ok(Tok::Star) => SyntaxKind::STAR,
            Ok(Tok::Slash) => SyntaxKind::SLASH,
            Ok(Tok::Percent) => SyntaxKind::PERCENT,
            Ok(Tok::Dot) => SyntaxKind::DOT,
            Err(()) => SyntaxKind::ERROR_TOKEN,
        };
        out.push(Token { kind, text });
    }
    out
}
