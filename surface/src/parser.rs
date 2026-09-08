//! The parser (docs/ROADMAP.md 6.3): recursive descent over the lexer's
//! tokens, building rowan's lossless green tree, with recovery at the
//! statement: an error is reported as E0101 (expected, found), the rest of
//! the line lands in an `ERROR` node, and parsing resumes at the next
//! line. A missing version marker or a newer one is E0105.
//!
//! The tree is generic where the grammar is regular: a keyword line
//! (`footprint owned: file("/x"), derived: p`) is a `LINE` whose keyword
//! is its first token and whose `ARGS` are keyword arguments and
//! expressions; a block (`site do ... end`) is a `BLOCK`; a definition is a
//! `DEF` with a `HEADER` (the atom, a `PATTERN` for `defop` and `defplan`,
//! keyword arguments) and a `BODY`. The resolver (unit B) gives each line
//! its meaning; the parser only shapes it.

use rowan::{Checkpoint, GreenNode, GreenNodeBuilder};
use rue_core::diagnostics::{Code, Diagnostic, Span};

use crate::lexer::{lex, Token};
use crate::syntax::SyntaxKind::{self, *};

/// The language version this compiler reads.
pub const LANGUAGE_VERSION: u32 = 0;

const DEFS: &[&str] = &[
    "defprobe",
    "defprim",
    "defop",
    "defplan",
    "defrole",
    "defprotocol",
    "defimpl",
];
const BLOCKS: &[&str] = &["site", "operators", "hooks", "par", "preflight"];

pub struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    /// Byte offset of each token.
    offsets: Vec<usize>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    diagnostics: Vec<Diagnostic>,
    file: String,
    src: &'a str,
    /// Set by a recovery until the next newline: a second error on the
    /// same line is a consequence of the first and is not reported.
    suppress: bool,
}

pub struct Parsed {
    pub green: GreenNode,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parse one expression standing alone (a string interpolation's `#{...}`).
/// `None` when the text is not exactly one expression.
pub fn parse_expr(src: &str) -> Option<crate::ast::Expr> {
    let tokens = lex(src);
    let mut offsets = Vec::with_capacity(tokens.len());
    let mut o = 0;
    for t in &tokens {
        offsets.push(o);
        o += t.text.len();
    }
    let mut p = Parser {
        tokens,
        offsets,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        diagnostics: Vec::new(),
        file: String::new(),
        src,
        suppress: false,
    };
    p.builder.start_node(ARGS.into());
    p.expr();
    p.eat_trivia();
    let complete = p.at_eof();
    p.builder.finish_node();
    if !p.diagnostics.is_empty() || !complete {
        return None;
    }
    let root = crate::syntax::SyntaxNode::new_root(p.builder.finish());
    root.children().next().map(|c| crate::ast::lower_expr(&c))
}

pub fn parse(src: &str, file: &str) -> Parsed {
    let tokens = lex(src);
    let mut offsets = Vec::with_capacity(tokens.len());
    let mut o = 0;
    for t in &tokens {
        offsets.push(o);
        o += t.text.len();
    }
    let mut p = Parser {
        tokens,
        offsets,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        diagnostics: Vec::new(),
        file: file.to_string(),
        src,
        suppress: false,
    };
    p.file_();
    Parsed {
        green: p.builder.finish(),
        diagnostics: p.diagnostics,
    }
}

impl<'a> Parser<'a> {
    // --- the token stream -------------------------------------------------

    fn is_trivia_here(&self, i: usize) -> bool {
        match self.tokens.get(i) {
            Some(t) => matches!(t.kind, WHITESPACE | COMMENT),
            None => false,
        }
    }

    /// Emit trivia tokens up to the next significant one.
    fn eat_trivia(&mut self) {
        while self.is_trivia_here(self.pos) {
            let t = self.tokens[self.pos];
            self.builder.token(t.kind.into(), t.text);
            self.pos += 1;
        }
    }

    fn peek_index(&self) -> usize {
        let mut i = self.pos;
        while self.is_trivia_here(i) {
            i += 1;
        }
        i
    }

    fn peek(&self) -> Option<Token<'a>> {
        self.tokens.get(self.peek_index()).copied()
    }

    fn peek_kind(&self) -> Option<SyntaxKind> {
        self.peek().map(|t| t.kind)
    }

    /// The significant token after the next one.
    fn peek2(&self) -> Option<Token<'a>> {
        let mut i = self.peek_index() + 1;
        while self.is_trivia_here(i) {
            i += 1;
        }
        self.tokens.get(i).copied()
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.peek_kind() == Some(kind)
    }

    fn at_name(&self, text: &str) -> bool {
        matches!(self.peek(), Some(t) if t.kind == NAME && t.text == text)
    }

    fn at_eof(&self) -> bool {
        self.peek().is_none()
    }

    /// Consume the next significant token into the tree.
    fn bump(&mut self) {
        self.eat_trivia();
        if let Some(t) = self.tokens.get(self.pos).copied() {
            if t.kind == NEWLINE {
                self.suppress = false;
            }
            self.builder.token(t.kind.into(), t.text);
            self.pos += 1;
        }
    }

    fn start(&mut self, kind: SyntaxKind) {
        self.eat_trivia();
        self.builder.start_node(kind.into());
    }

    fn checkpoint(&mut self) -> Checkpoint {
        self.eat_trivia();
        self.builder.checkpoint()
    }

    fn start_at(&mut self, cp: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(cp, kind.into());
    }

    fn finish(&mut self) {
        self.builder.finish_node();
    }

    // --- diagnostics ------------------------------------------------------

    fn span_at(&self, index: usize) -> Span {
        let offset = self.offsets.get(index).copied().unwrap_or(self.src.len());
        let before = &self.src[..offset.min(self.src.len())];
        let line = before.matches('\n').count() as u32 + 1;
        let col = before
            .rsplit('\n')
            .next()
            .map(|l| l.chars().count())
            .unwrap_or(0) as u32
            + 1;
        Span {
            file: self.file.clone(),
            line,
            col,
        }
    }

    fn found_text(&self) -> String {
        match self.peek() {
            None => "end of file".to_string(),
            Some(t) => match t.kind {
                NEWLINE => "end of line".to_string(),
                NAME | UPPER_NAME | ATOM | INT | FLOAT | DURATION | STRING => {
                    format!("`{}`", t.text)
                }
                ERROR_TOKEN => format!("`{}`", t.text),
                k => k.describe().to_string(),
            },
        }
    }

    /// Report E0101 at the next significant token and swallow the rest of
    /// the line into an `ERROR` node so parsing resumes at the next line.
    fn error(&mut self, expected: &str, context: &str) {
        if self.suppress {
            return;
        }
        self.suppress = true;
        let span = self.span_at(self.peek_index());
        let found = self.found_text();
        self.diagnostics.push(Diagnostic {
            code: Code::E0101,
            span: Some(span),
            expected: Some(expected.to_string()),
            found: Some(found),
            nearest: None,
            message: format!("parse error {context}"),
        });
        self.start(ERROR);
        while let Some(t) = self.peek() {
            if t.kind == NEWLINE {
                break;
            }
            self.bump();
        }
        self.finish();
    }

    /// A body ran into a top-level keyword: the enclosing block was never
    /// closed. Reported here, at the keyword, and the enclosing `end` check
    /// is quieted so the keyword's own line parses as the top-level form it
    /// is.
    fn missing_end(&mut self, context: &str) {
        if self.suppress {
            return;
        }
        let span = self.span_at(self.peek_index());
        let found = self.found_text();
        self.diagnostics.push(Diagnostic {
            code: Code::E0101,
            span: Some(span),
            expected: Some("`end`".to_string()),
            found: Some(found),
            nearest: None,
            message: format!("parse error {context}"),
        });
        self.suppress = true;
    }

    /// The next token must be `kind`; else E0101 and line recovery.
    fn expect(&mut self, kind: SyntaxKind, context: &str) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            self.error(kind.describe(), context);
            false
        }
    }

    fn expect_name(&mut self, text: &str, context: &str) -> bool {
        if self.at_name(text) {
            self.bump();
            true
        } else {
            self.error(&format!("`{text}`"), context);
            false
        }
    }

    /// End of a statement: a newline (or the end of the file).
    fn expect_newline(&mut self, context: &str) {
        if self.at_eof() {
            return;
        }
        if !self.at(NEWLINE) {
            self.error("end of line", context);
        }
        if self.at(NEWLINE) {
            self.bump();
        }
    }

    // --- the file ---------------------------------------------------------

    fn file_(&mut self) {
        self.builder.start_node(FILE.into());
        self.version();
        while !self.at_eof() {
            self.top();
        }
        self.eat_trivia();
        self.finish();
    }

    /// `rue INT`, first (E0105 when missing or newer).
    fn version(&mut self) {
        if self.at_name("rue") {
            let idx = self.peek_index();
            self.start(VERSION);
            self.bump();
            if self.at(INT) {
                let v: u32 = self.peek().unwrap().text.parse().unwrap_or(u32::MAX);
                if v > LANGUAGE_VERSION {
                    self.diagnostics.push(Diagnostic {
                        code: Code::E0105,
                        span: Some(self.span_at(idx)),
                        expected: Some(format!("`rue {LANGUAGE_VERSION}`")),
                        found: Some(format!("`rue {v}`")),
                        nearest: None,
                        message: format!(
                            "language version {v} is newer than this compiler reads ({LANGUAGE_VERSION})"
                        ),
                    });
                }
                self.bump();
                self.expect_newline("after the version marker");
            } else {
                self.error("an integer", "after `rue`");
                if self.at(NEWLINE) {
                    self.bump();
                }
            }
            self.finish();
        } else {
            self.diagnostics.push(Diagnostic {
                code: Code::E0105,
                span: Some(self.span_at(0)),
                expected: Some(format!("`rue {LANGUAGE_VERSION}` on the first line")),
                found: None,
                nearest: None,
                message: "language version marker missing".to_string(),
            });
        }
    }

    fn top(&mut self) {
        if self.at(NEWLINE) {
            self.bump();
            return;
        }
        match self.peek() {
            Some(t) if t.kind == NAME && DEFS.contains(&t.text) => self.def(),
            Some(t) if t.kind == NAME && t.text == "site" => self.block(),
            Some(t) if t.kind == NAME && t.text == "import" => self.import(),
            _ => self.error(
                "a definition, `site`, or `import`",
                "at the top of the file",
            ),
        }
        if self.at(NEWLINE) {
            self.bump();
        }
    }

    fn import(&mut self) {
        self.start(IMPORT);
        self.bump();
        if self.expect(STRING, "after `import`") && self.at_name("as") {
            self.bump();
            self.expect(NAME, "after `as`");
        }
        self.expect_newline("after the import");
        self.finish();
    }

    /// `KEYWORD do NEWLINE body end NEWLINE`.
    fn block(&mut self) {
        self.start(BLOCK);
        self.bump();
        if self.expect_name("do", "after the block keyword") {
            self.expect_newline("after `do`");
            self.body();
            self.expect_name("end", "to close the block");
            self.expect_newline("after `end`");
        }
        self.finish();
    }

    /// `defX :name (, pattern)? (, kw)* do NEWLINE body end NEWLINE`.
    fn def(&mut self) {
        self.start(DEF);
        let keyword = self.peek().unwrap().text;
        let patterned = keyword == "defop" || keyword == "defplan";
        self.bump();
        self.start(HEADER);
        let ok = self.expect(ATOM, "after the definition keyword");
        let mut first = true;
        while ok && self.at(COMMA) {
            self.bump();
            if first && patterned {
                self.pattern();
            } else {
                self.arg();
            }
            first = false;
        }
        self.finish();
        if ok && self.expect_name("do", "after the definition's header") {
            self.expect_newline("after `do`");
            self.body();
            self.expect_name("end", "to close the definition");
            self.expect_newline("after `end`");
        }
        self.finish();
    }

    /// A clause pattern: an expression (a record, `_`, a name) with an
    /// optional `= name` capture.
    fn pattern(&mut self) {
        self.start(PATTERN);
        self.expr();
        if self.at(EQ) {
            self.bump();
            self.expect(NAME, "after `=` in a pattern");
        }
        self.finish();
    }

    /// Statements until `end` or `else` (not consumed) or the end of file.
    fn body(&mut self) {
        self.start(BODY);
        loop {
            if self.at_eof() || self.at_name("end") || self.at_name("else") {
                break;
            }
            if self.at(NEWLINE) {
                self.bump();
                continue;
            }
            // A definition or an import belongs at the top of the file: the
            // block above it was never closed.
            if let Some(t) = self.peek() {
                if t.kind == NAME
                    && (DEFS.contains(&t.text) || t.text == "import" || t.text == "site")
                {
                    self.missing_end("to close the block before this definition");
                    break;
                }
            }
            let before = self.pos;
            self.stmt();
            // Progress is guaranteed: a statement that consumed nothing (an
            // error while a recovery is still quieting the line) is stepped
            // over one token at a time, so no input can stall the parser. A
            // mutant that never cleared the quiet flag hung the suite here.
            if self.pos == before {
                self.start(ERROR);
                self.bump();
                self.finish();
            }
        }
        self.eat_trivia();
        self.finish();
    }

    fn stmt(&mut self) {
        let Some(t) = self.peek() else { return };
        if t.kind == ATOM {
            // A defrole contribution: `:slot (INT)? item`.
            self.start(CONTRIBUTION);
            self.bump();
            if self.at(INT) {
                self.bump();
            }
            if matches!(self.peek_kind(), Some(NAME)) {
                self.stmt();
            } else {
                self.error("an item", "after the slot name");
                if self.at(NEWLINE) {
                    self.bump();
                }
            }
            self.finish();
            return;
        }
        if t.kind != NAME {
            self.error("a statement", "in a body");
            if self.at(NEWLINE) {
                self.bump();
            }
            return;
        }
        match t.text {
            k if BLOCKS.contains(&k) => self.block(),
            "knell" => {
                self.start(KNELL);
                self.bump();
                self.step_or_pipeline();
                self.finish();
            }
            "slot" => {
                self.start(SLOT);
                self.bump();
                self.expect(ATOM, "after `slot`");
                self.expect_newline("after the slot name");
                self.finish();
            }
            "observe" => {
                self.start(OBSERVE);
                self.bump();
                self.expr();
                if self.expect_name("as", "after the probe call") {
                    self.expect(NAME, "after `as`");
                }
                self.expect_newline("after the observe");
                self.finish();
            }
            "assert" => {
                self.start(ASSERT);
                self.bump();
                self.args();
                self.expect_newline("after the assertion");
                self.finish();
            }
            "repeat" => self.repeat(),
            "when" => self.when(),
            _ => {
                let next = self.peek2().map(|t| t.kind);
                if matches!(next, Some(L_PAREN) | Some(DOT)) {
                    self.step_or_pipeline();
                } else {
                    self.line();
                }
            }
        }
    }

    /// A keyword line: `keyword (:)? args NEWLINE`.
    fn line(&mut self) {
        self.start(LINE);
        self.bump();
        if self.at(COLON) {
            self.bump();
        }
        if !self.at(NEWLINE) && !self.at_eof() {
            self.args();
        }
        self.expect_newline("after the line");
        self.finish();
    }

    fn repeat(&mut self) {
        self.start(REPEAT);
        self.bump();
        // `repeat INT as NAME do` or `repeat over: expr, as NAME, max: INT do`.
        while !self.at_name("do") && !self.at(NEWLINE) && !self.at_eof() {
            if self.at(COMMA) {
                self.bump();
            } else if self.at_name("as") {
                self.bump();
                self.expect(NAME, "after `as`");
            } else {
                self.arg();
            }
        }
        if self.expect_name("do", "after the repeat header") {
            self.expect_newline("after `do`");
            self.body();
            self.expect_name("end", "to close the repeat");
            self.expect_newline("after `end`");
        }
        self.finish();
    }

    fn when(&mut self) {
        self.start(WHEN);
        self.bump();
        self.args();
        if self.expect_name("do", "after the when guard") {
            self.expect_newline("after `do`");
            self.body();
            if self.at_name("else") {
                self.start(ELSE_ARM);
                self.bump();
                self.expect_newline("after `else`");
                self.body();
                self.finish();
            }
            self.expect_name("end", "to close the when");
            self.expect_newline("after `end`");
        }
        self.finish();
    }

    /// `step (|> step)* NEWLINE`.
    fn step_or_pipeline(&mut self) {
        let cp = self.checkpoint();
        self.step();
        if self.at(PIPE_GT) {
            self.start_at(cp, PIPELINE);
            while self.at(PIPE_GT) {
                self.bump();
                self.step();
            }
            self.finish();
        }
        self.expect_newline("after the step");
    }

    /// `call (, kw)* (as NAME)?` without its newline.
    fn step(&mut self) {
        self.start(STEP);
        self.postfix();
        while self.at(COMMA) {
            self.bump();
            self.arg();
        }
        if self.at_name("as") {
            self.bump();
            self.expect(NAME, "after `as`");
        }
        self.finish();
    }

    // --- arguments and expressions ---------------------------------------

    /// `arg (, arg)*`.
    fn args(&mut self) {
        self.start(ARGS);
        self.arg();
        while self.at(COMMA) {
            self.bump();
            self.arg();
        }
        self.finish();
    }

    /// A keyword argument (`name: arg`, nesting allowed) or an expression.
    fn arg(&mut self) {
        let is_key = matches!(
            self.peek_kind(),
            Some(NAME) | Some(UPPER_NAME) | Some(STRING)
        ) && self.peek2().map(|t| t.kind) == Some(COLON);
        if is_key {
            self.start(KW);
            self.bump();
            self.bump();
            self.arg();
            self.finish();
        } else {
            self.expr();
        }
    }

    fn expr(&mut self) {
        self.or_();
    }

    fn binary(&mut self, cp: Checkpoint) {
        self.start_at(cp, BINARY);
    }

    fn or_(&mut self) {
        let cp = self.checkpoint();
        self.and_();
        while self.at_name("or") {
            self.binary(cp);
            self.bump();
            self.and_();
            self.finish();
        }
    }

    fn and_(&mut self) {
        let cp = self.checkpoint();
        self.not_();
        while self.at_name("and") {
            self.binary(cp);
            self.bump();
            self.not_();
            self.finish();
        }
    }

    fn not_(&mut self) {
        if self.at_name("not") {
            self.start(UNARY);
            self.bump();
            self.not_();
            self.finish();
        } else {
            self.cmp();
        }
    }

    fn cmp(&mut self) {
        let cp = self.checkpoint();
        self.add();
        if matches!(self.peek_kind(), Some(LT | LE | GT | GE | EQ_EQ | BANG_EQ)) {
            self.binary(cp);
            self.bump();
            self.add();
            self.finish();
        }
    }

    fn add(&mut self) {
        let cp = self.checkpoint();
        self.mul();
        while matches!(self.peek_kind(), Some(PLUS | MINUS)) {
            self.binary(cp);
            self.bump();
            self.mul();
            self.finish();
        }
    }

    fn mul(&mut self) {
        let cp = self.checkpoint();
        self.unary();
        while matches!(self.peek_kind(), Some(STAR | SLASH | PERCENT)) {
            self.binary(cp);
            self.bump();
            self.unary();
            self.finish();
        }
    }

    fn unary(&mut self) {
        if self.at(MINUS) {
            self.start(UNARY);
            self.bump();
            self.unary();
            self.finish();
        } else {
            self.postfix();
        }
    }

    /// An atom, and a call when `(` follows a reference.
    fn postfix(&mut self) {
        match self.peek_kind() {
            Some(STRING) => {
                let text = self.peek().unwrap().text;
                if text.len() < 2 || !text.ends_with('"') {
                    self.error("a closing quote", "in a string");
                    return;
                }
                self.start(LITERAL);
                self.bump();
                self.finish();
            }
            Some(INT | FLOAT | ATOM | DURATION) => {
                self.start(LITERAL);
                self.bump();
                self.finish();
            }
            Some(NAME) => {
                let t = self.peek().unwrap();
                if t.text == "true" || t.text == "false" {
                    self.start(LITERAL);
                    self.bump();
                    self.finish();
                    return;
                }
                let cp = self.checkpoint();
                self.start(REF);
                self.bump();
                while self.at(DOT) {
                    self.bump();
                    self.expect(NAME, "after `.`");
                }
                self.finish();
                if self.at(L_PAREN) {
                    self.start_at(cp, CALL);
                    self.bump();
                    if !self.at(R_PAREN) {
                        self.args();
                    }
                    self.expect(R_PAREN, "to close the call");
                    self.finish();
                }
            }
            Some(UPPER_NAME) => {
                self.start(REF);
                self.bump();
                self.finish();
            }
            Some(L_PAREN) => {
                self.start(PAREN);
                self.bump();
                self.expr();
                self.expect(R_PAREN, "to close the parenthesis");
                self.finish();
            }
            Some(L_BRACK) => {
                self.start(LIST);
                self.bump();
                if !self.at(R_BRACK) {
                    self.arg();
                    while self.at(COMMA) {
                        self.bump();
                        self.arg();
                    }
                }
                self.expect(R_BRACK, "to close the list");
                self.finish();
            }
            Some(PERCENT_BRACE) => {
                self.start(RECORD);
                self.bump();
                if !self.at(R_BRACE) {
                    self.record_entry();
                    while self.at(COMMA) {
                        self.bump();
                        self.record_entry();
                    }
                }
                self.expect(R_BRACE, "to close the record");
                self.finish();
            }
            _ => self.error("an expression", "here"),
        }
    }

    fn record_entry(&mut self) {
        self.start(RECORD_ENTRY);
        if matches!(self.peek_kind(), Some(NAME | UPPER_NAME | STRING)) {
            self.bump();
            if self.expect(COLON, "after the record key") {
                self.arg();
            }
        } else {
            self.error("a record key", "in a record");
        }
        self.finish();
    }
}
