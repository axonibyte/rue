//! The tree as data (docs/ROADMAP.md 6.3): plain structures lowered from
//! the lossless tree, each carrying the text range it came from so the
//! resolver can place a diagnostic. Lowering never fails: a tree with
//! `ERROR` nodes lowers to what parsed, and the parser's diagnostics say
//! the rest. Strings stay raw here (quotes and escapes as written); the
//! resolver reads their parts.

use rowan::TextRange;

use crate::syntax::SyntaxKind::{self, *};
use crate::syntax::{SyntaxElement, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub version: Option<u32>,
    pub tops: Vec<Top>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Top {
    Site(Block),
    Import(Import),
    Def(Def),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub range: TextRange,
    /// The path, unquoted.
    pub path: String,
    pub alias: Option<String>,
}

/// `keyword do stmts end`.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub range: TextRange,
    pub keyword: String,
    pub body: Vec<Stmt>,
}

/// `defX :name (, pattern)? (, params)* do stmts end`.
#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    pub range: TextRange,
    pub keyword: String,
    /// The atom without its colon.
    pub name: String,
    pub name_range: TextRange,
    pub pattern: Option<Pattern>,
    pub params: Vec<Kw>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub range: TextRange,
    pub expr: Expr,
    pub capture: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Line(Line),
    Block(Block),
    Def(Def),
    Import(Import),
    Step(Step),
    Pipeline(Vec<Step>),
    Knell(Step),
    Slot {
        range: TextRange,
        name: String,
    },
    Observe {
        range: TextRange,
        call: Expr,
        alias: Option<String>,
    },
    Assert {
        range: TextRange,
        args: Vec<Arg>,
    },
    Repeat {
        range: TextRange,
        args: Vec<Arg>,
        var: Option<String>,
        body: Vec<Stmt>,
    },
    When {
        range: TextRange,
        args: Vec<Arg>,
        then_: Vec<Stmt>,
        else_: Option<Vec<Stmt>>,
    },
    Contribution {
        range: TextRange,
        slot: String,
        priority: Option<u32>,
        item: Box<Stmt>,
    },
}

/// `keyword (:)? args`.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub range: TextRange,
    pub keyword: String,
    pub args: Vec<Arg>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub range: TextRange,
    pub call: Expr,
    pub kws: Vec<Kw>,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Kw(Kw),
    Expr(Expr),
}

impl Arg {
    pub fn range(&self) -> TextRange {
        match self {
            Arg::Kw(k) => k.range,
            Arg::Expr(e) => e.range(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Kw {
    pub range: TextRange,
    /// The key as written: a name, an upper-case name, or a string with its quotes.
    pub name: String,
    pub value: Box<Arg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit {
        range: TextRange,
        lit: Lit,
    },
    /// `a.b.c` as its segments.
    Ref {
        range: TextRange,
        path: Vec<String>,
    },
    Call {
        range: TextRange,
        path: Vec<String>,
        args: Vec<Arg>,
    },
    Binary {
        range: TextRange,
        op: String,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        range: TextRange,
        op: String,
        expr: Box<Expr>,
    },
    Paren {
        range: TextRange,
        expr: Box<Expr>,
    },
    List {
        range: TextRange,
        items: Vec<Arg>,
    },
    Record {
        range: TextRange,
        entries: Vec<Kw>,
    },
    /// A node that did not parse (the parser reported it).
    Error {
        range: TextRange,
    },
}

impl Expr {
    pub fn range(&self) -> TextRange {
        match self {
            Expr::Lit { range, .. }
            | Expr::Ref { range, .. }
            | Expr::Call { range, .. }
            | Expr::Binary { range, .. }
            | Expr::Unary { range, .. }
            | Expr::Paren { range, .. }
            | Expr::List { range, .. }
            | Expr::Record { range, .. }
            | Expr::Error { range } => *range,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Int(u64),
    /// Kept as written.
    Float(String),
    /// Raw, with quotes and escapes.
    Str(String),
    /// Without the colon and without quotes.
    Atom(String),
    /// Seconds.
    Duration(u64),
    Bool(bool),
}

// ---------------------------------------------------------------------------

/// Lower a parsed tree.
pub fn lower(root: &SyntaxNode) -> File {
    let mut version = None;
    let mut tops = Vec::new();
    for child in root.children() {
        match child.kind() {
            VERSION => {
                version = tokens(&child)
                    .into_iter()
                    .find(|t| t.kind() == INT)
                    .and_then(|t| t.text().parse().ok());
            }
            BLOCK => tops.push(Top::Site(block(&child))),
            IMPORT => tops.push(Top::Import(import(&child))),
            DEF => tops.push(Top::Def(def(&child))),
            _ => {}
        }
    }
    File { version, tops }
}

fn tokens(n: &SyntaxNode) -> Vec<SyntaxToken> {
    n.children_with_tokens()
        .filter_map(|e| match e {
            SyntaxElement::Token(t) if !t.kind().is_trivia() => Some(t),
            _ => None,
        })
        .collect()
}

fn names(n: &SyntaxNode) -> Vec<String> {
    tokens(n)
        .into_iter()
        .filter(|t| t.kind() == NAME)
        .map(|t| t.text().to_string())
        .collect()
}

fn child(n: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    n.children().find(|c| c.kind() == kind)
}

fn atom_text(t: &SyntaxToken) -> String {
    let s = t.text().trim_start_matches(':');
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn import(n: &SyntaxNode) -> Import {
    let toks = tokens(n);
    let path = toks
        .iter()
        .find(|t| t.kind() == STRING)
        .map(|t| unquote(t.text()))
        .unwrap_or_default();
    let alias = toks
        .iter()
        .position(|t| t.kind() == NAME && t.text() == "as")
        .and_then(|i| toks.get(i + 1))
        .filter(|t| t.kind() == NAME)
        .map(|t| t.text().to_string());
    Import {
        range: n.text_range(),
        path,
        alias,
    }
}

/// The text of a string token without its quotes and with `\"` and `\\`
/// unescaped; an unterminated string is returned as written.
pub fn unquote(raw: &str) -> String {
    let inner = raw.strip_prefix('"').unwrap_or(raw);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    let mut out = String::with_capacity(inner.len());
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn block(n: &SyntaxNode) -> Block {
    Block {
        range: n.text_range(),
        keyword: names(n).first().cloned().unwrap_or_default(),
        body: child(n, BODY).map(|b| body(&b)).unwrap_or_default(),
    }
}

fn def(n: &SyntaxNode) -> Def {
    let keyword = names(n).first().cloned().unwrap_or_default();
    let header = child(n, HEADER);
    let (name, name_range) = header
        .as_ref()
        .and_then(|h| tokens(h).into_iter().find(|t| t.kind() == ATOM))
        .map(|t| (atom_text(&t), t.text_range()))
        .unwrap_or((String::new(), n.text_range()));
    let mut pattern = None;
    let mut params = Vec::new();
    if let Some(h) = &header {
        for c in h.children() {
            match c.kind() {
                PATTERN => pattern = Some(pattern_of(&c)),
                KW => params.push(kw(&c)),
                k if is_expr_kind(k) => params.push(Kw {
                    range: c.text_range(),
                    name: String::new(),
                    value: Box::new(Arg::Expr(expr(&c))),
                }),
                _ => {}
            }
        }
    }
    Def {
        range: n.text_range(),
        keyword,
        name,
        name_range,
        pattern,
        params,
        body: child(n, BODY).map(|b| body(&b)).unwrap_or_default(),
    }
}

fn pattern_of(n: &SyntaxNode) -> Pattern {
    let e = n
        .children()
        .find(|c| is_expr_kind(c.kind()))
        .map(|c| expr(&c))
        .unwrap_or(Expr::Error {
            range: n.text_range(),
        });
    let toks = tokens(n);
    let capture = toks
        .iter()
        .position(|t| t.kind() == EQ)
        .and_then(|i| toks.get(i + 1))
        .map(|t| t.text().to_string());
    Pattern {
        range: n.text_range(),
        expr: e,
        capture,
    }
}

fn body(n: &SyntaxNode) -> Vec<Stmt> {
    n.children().filter_map(|c| stmt(&c)).collect()
}

fn stmt(n: &SyntaxNode) -> Option<Stmt> {
    Some(match n.kind() {
        LINE => Stmt::Line(line(n)),
        BLOCK => Stmt::Block(block(n)),
        DEF => Stmt::Def(def(n)),
        IMPORT => Stmt::Import(import(n)),
        STEP => Stmt::Step(step(n)),
        PIPELINE => Stmt::Pipeline(
            n.children()
                .filter(|c| c.kind() == STEP)
                .map(|c| step(&c))
                .collect(),
        ),
        KNELL => Stmt::Knell(child(n, STEP).map(|s| step(&s))?),
        SLOT => Stmt::Slot {
            range: n.text_range(),
            name: tokens(n)
                .into_iter()
                .find(|t| t.kind() == ATOM)
                .map(|t| atom_text(&t))
                .unwrap_or_default(),
        },
        OBSERVE => Stmt::Observe {
            range: n.text_range(),
            call: n
                .children()
                .find(|c| is_expr_kind(c.kind()))
                .map(|c| expr(&c))
                .unwrap_or(Expr::Error {
                    range: n.text_range(),
                }),
            alias: {
                let toks = tokens(n);
                toks.iter()
                    .position(|t| t.kind() == NAME && t.text() == "as")
                    .and_then(|i| toks.get(i + 1))
                    .map(|t| t.text().to_string())
            },
        },
        ASSERT => Stmt::Assert {
            range: n.text_range(),
            args: child(n, ARGS).map(|a| args(&a)).unwrap_or_default(),
        },
        REPEAT => {
            let toks = tokens(n);
            let var = toks
                .iter()
                .position(|t| t.kind() == NAME && t.text() == "as")
                .and_then(|i| toks.get(i + 1))
                .map(|t| t.text().to_string());
            let mut hargs = Vec::new();
            for c in n.children() {
                match c.kind() {
                    KW => hargs.push(Arg::Kw(kw(&c))),
                    k if is_expr_kind(k) => hargs.push(Arg::Expr(expr(&c))),
                    _ => {}
                }
            }
            Stmt::Repeat {
                range: n.text_range(),
                args: hargs,
                var,
                body: child(n, BODY).map(|b| body(&b)).unwrap_or_default(),
            }
        }
        WHEN => {
            let bodies: Vec<SyntaxNode> = n.children().filter(|c| c.kind() == BODY).collect();
            Stmt::When {
                range: n.text_range(),
                args: child(n, ARGS).map(|a| args(&a)).unwrap_or_default(),
                then_: bodies.first().map(body).unwrap_or_default(),
                else_: child(n, ELSE_ARM)
                    .and_then(|e| child(&e, BODY))
                    .map(|b| body(&b)),
            }
        }
        CONTRIBUTION => {
            let toks = tokens(n);
            Stmt::Contribution {
                range: n.text_range(),
                slot: toks
                    .iter()
                    .find(|t| t.kind() == ATOM)
                    .map(atom_text)
                    .unwrap_or_default(),
                priority: toks
                    .iter()
                    .find(|t| t.kind() == INT)
                    .and_then(|t| t.text().parse().ok()),
                item: Box::new(n.children().find_map(|c| stmt(&c))?),
            }
        }
        _ => return None,
    })
}

fn line(n: &SyntaxNode) -> Line {
    let toks = tokens(n);
    Line {
        range: n.text_range(),
        keyword: toks
            .first()
            .map(|t| t.text().to_string())
            .unwrap_or_default(),
        args: child(n, ARGS).map(|a| args(&a)).unwrap_or_default(),
    }
}

fn step(n: &SyntaxNode) -> Step {
    let mut call = None;
    let mut kws = Vec::new();
    for c in n.children() {
        match c.kind() {
            KW => kws.push(kw(&c)),
            k if is_expr_kind(k) && call.is_none() => call = Some(expr(&c)),
            _ => {}
        }
    }
    let toks = tokens(n);
    let alias = toks
        .iter()
        .position(|t| t.kind() == NAME && t.text() == "as")
        .and_then(|i| toks.get(i + 1))
        .map(|t| t.text().to_string());
    Step {
        range: n.text_range(),
        call: call.unwrap_or(Expr::Error {
            range: n.text_range(),
        }),
        kws,
        alias,
    }
}

fn args(n: &SyntaxNode) -> Vec<Arg> {
    n.children()
        .filter_map(|c| match c.kind() {
            KW => Some(Arg::Kw(kw(&c))),
            k if is_expr_kind(k) => Some(Arg::Expr(expr(&c))),
            _ => None,
        })
        .collect()
}

fn kw(n: &SyntaxNode) -> Kw {
    let name = tokens(n)
        .first()
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    let value = n
        .children()
        .find_map(|c| match c.kind() {
            KW => Some(Arg::Kw(kw(&c))),
            k if is_expr_kind(k) => Some(Arg::Expr(expr(&c))),
            _ => None,
        })
        .unwrap_or(Arg::Expr(Expr::Error {
            range: n.text_range(),
        }));
    Kw {
        range: n.text_range(),
        name,
        value: Box::new(value),
    }
}

fn is_expr_kind(k: SyntaxKind) -> bool {
    matches!(
        k,
        LITERAL | REF | CALL | BINARY | UNARY | PAREN | LIST | RECORD | ERROR
    )
}

fn expr(n: &SyntaxNode) -> Expr {
    let range = n.text_range();
    match n.kind() {
        LITERAL => {
            let t = tokens(n).into_iter().next();
            let lit = match t {
                Some(t) => match t.kind() {
                    INT => Lit::Int(t.text().parse().unwrap_or(u64::MAX)),
                    FLOAT => Lit::Float(t.text().to_string()),
                    STRING => Lit::Str(t.text().to_string()),
                    ATOM => Lit::Atom(atom_text(&t)),
                    DURATION => Lit::Duration(duration_seconds(t.text())),
                    NAME => Lit::Bool(t.text() == "true"),
                    _ => Lit::Str(String::new()),
                },
                None => Lit::Str(String::new()),
            };
            Expr::Lit { range, lit }
        }
        REF => Expr::Ref {
            range,
            path: tokens(n)
                .iter()
                .filter(|t| matches!(t.kind(), NAME | UPPER_NAME))
                .map(|t| t.text().to_string())
                .collect(),
        },
        CALL => {
            let path = child(n, REF).map(|r| names(&r)).unwrap_or_default();
            Expr::Call {
                range,
                path,
                args: child(n, ARGS).map(|a| args(&a)).unwrap_or_default(),
            }
        }
        BINARY => {
            let operands: Vec<Expr> = n
                .children()
                .filter(|c| is_expr_kind(c.kind()))
                .map(|c| expr(&c))
                .collect();
            let op = tokens(n)
                .first()
                .map(|t| t.text().to_string())
                .unwrap_or_default();
            let mut it = operands.into_iter();
            let lhs = it.next().unwrap_or(Expr::Error { range });
            let rhs = it.next().unwrap_or(Expr::Error { range });
            Expr::Binary {
                range,
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }
        }
        UNARY => Expr::Unary {
            range,
            op: tokens(n)
                .first()
                .map(|t| t.text().to_string())
                .unwrap_or_default(),
            expr: Box::new(
                n.children()
                    .find(|c| is_expr_kind(c.kind()))
                    .map(|c| expr(&c))
                    .unwrap_or(Expr::Error { range }),
            ),
        },
        PAREN => Expr::Paren {
            range,
            expr: Box::new(
                n.children()
                    .find(|c| is_expr_kind(c.kind()))
                    .map(|c| expr(&c))
                    .unwrap_or(Expr::Error { range }),
            ),
        },
        LIST => Expr::List {
            range,
            items: n
                .children()
                .filter_map(|c| match c.kind() {
                    KW => Some(Arg::Kw(kw(&c))),
                    k if is_expr_kind(k) => Some(Arg::Expr(expr(&c))),
                    _ => None,
                })
                .collect(),
        },
        RECORD => Expr::Record {
            range,
            entries: n
                .children()
                .filter(|c| c.kind() == RECORD_ENTRY)
                .map(|c| kw(&c))
                .collect(),
        },
        _ => Expr::Error { range },
    }
}

/// Lower one expression node.
pub fn lower_expr(n: &SyntaxNode) -> Expr {
    expr(n)
}

/// `30m` as seconds; the lexer admits only `INT unit`.
pub fn duration_seconds(text: &str) -> u64 {
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u64 = digits.parse().unwrap_or(0);
    match &text[digits.len()..] {
        "ms" => n / 1000,
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86_400,
        _ => n,
    }
}
