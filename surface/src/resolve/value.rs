//! Static values (docs/ROADMAP.md 6.5): what an expression means at check
//! time when it means anything without a host. A reference to a name is
//! symbolic here; the expansion classifies it. `Unknown` and `Secret`
//! propagate through every operator, as section 5.1 requires, and a
//! comparison against `:unknown` is E0108.

use rue_core::model::Duration;

use crate::ast::{Arg, Expr, Kw, Lit};

/// A value an expression evaluates to without a host.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Int(i64),
    Float(f64),
    Str(String),
    Atom(String),
    Duration(Duration),
    Bool(bool),
    List(Vec<Val>),
    Record(Vec<(String, Val)>),
    /// A name (possibly dotted) whose value is bound elsewhere.
    Ref(Vec<String>),
    /// A call that is not a builtin: kept as its name and arguments.
    Call(Vec<String>, Vec<Arg>),
    /// A string with interpolations: literal parts and expression parts.
    Template(Vec<Part>),
    Unknown,
    /// A value no static evaluation reaches (a fact at run time).
    Opaque,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Lit(String),
    Expr(Expr),
}

/// The kind of a value (6.5), for E0107.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    Str,
    Atom,
    Duration,
    Bool,
    List,
    Record,
    Fact,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Int => "int",
            Kind::Float => "float",
            Kind::Str => "str",
            Kind::Atom => "atom",
            Kind::Duration => "duration",
            Kind::Bool => "bool",
            Kind::List => "list",
            Kind::Record => "record",
            Kind::Fact => "fact",
        }
    }
}

impl Val {
    pub fn kind(&self) -> Option<Kind> {
        Some(match self {
            Val::Int(_) => Kind::Int,
            Val::Float(_) => Kind::Float,
            Val::Str(_) | Val::Template(_) => Kind::Str,
            Val::Atom(_) => Kind::Atom,
            Val::Duration(_) => Kind::Duration,
            Val::Bool(_) => Kind::Bool,
            Val::List(_) => Kind::List,
            Val::Record(_) => Kind::Record,
            Val::Ref(_) | Val::Call(..) | Val::Unknown | Val::Opaque => return None,
        })
    }

    pub fn as_atom(&self) -> Option<&str> {
        match self {
            Val::Atom(a) => Some(a),
            _ => None,
        }
    }
}

/// The parts of a raw string token: literal text and `#{...}` expressions,
/// the expression text parsed on the spot.
pub fn string_parts(raw: &str, parse_expr: &dyn Fn(&str) -> Option<Expr>) -> Vec<Part> {
    let inner = crate::ast::unquote(raw);
    let mut parts = Vec::new();
    let mut lit = String::new();
    let bytes: Vec<char> = inner.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '#' && bytes.get(i + 1) == Some(&'{') {
            let mut depth = 1;
            let mut j = i + 2;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let expr_text: String = bytes[i + 2..j.saturating_sub(1)].iter().collect();
            if !lit.is_empty() {
                parts.push(Part::Lit(std::mem::take(&mut lit)));
            }
            match parse_expr(expr_text.trim()) {
                Some(e) => parts.push(Part::Expr(e)),
                None => parts.push(Part::Lit(format!("#{{{expr_text}}}"))),
            }
            i = j;
        } else {
            lit.push(bytes[i]);
            i += 1;
        }
    }
    if !lit.is_empty() || parts.is_empty() {
        parts.push(Part::Lit(lit));
    }
    parts
}

/// Evaluate an expression statically. References stay references; a
/// builtin over known values is computed; anything else is `Opaque`.
pub fn eval(e: &Expr, parse_expr: &dyn Fn(&str) -> Option<Expr>) -> Val {
    match e {
        Expr::Lit { lit, .. } => match lit {
            Lit::Int(n) => Val::Int(*n as i64),
            Lit::Float(f) => Val::Float(f.parse().unwrap_or(0.0)),
            Lit::Str(raw) => {
                let parts = string_parts(raw, parse_expr);
                if parts.iter().all(|p| matches!(p, Part::Lit(_))) {
                    Val::Str(
                        parts
                            .iter()
                            .map(|p| match p {
                                Part::Lit(s) => s.as_str(),
                                Part::Expr(_) => "",
                            })
                            .collect(),
                    )
                } else {
                    Val::Template(parts)
                }
            }
            Lit::Atom(a) => Val::Atom(a.clone()),
            Lit::Duration(s) => Val::Duration(Duration::new(*s)),
            Lit::Bool(b) => Val::Bool(*b),
        },
        Expr::Ref { path, .. } => Val::Ref(path.clone()),
        Expr::Call { path, args, .. } => Val::Call(path.clone(), args.clone()),
        Expr::Paren { expr, .. } => eval(expr, parse_expr),
        Expr::List { items, .. } => Val::List(
            items
                .iter()
                .map(|a| match a {
                    Arg::Expr(e) => eval(e, parse_expr),
                    Arg::Kw(k) => {
                        Val::Record(vec![(k.name.clone(), eval_arg(&k.value, parse_expr))])
                    }
                })
                .collect(),
        ),
        Expr::Record { entries, .. } => Val::Record(
            entries
                .iter()
                .map(|k: &Kw| (crate::ast::unquote(&k.name), eval_arg(&k.value, parse_expr)))
                .collect(),
        ),
        Expr::Unary { op, expr, .. } => match (op.as_str(), eval(expr, parse_expr)) {
            ("-", Val::Int(n)) => Val::Int(-n),
            ("-", Val::Float(f)) => Val::Float(-f),
            ("not", Val::Bool(b)) => Val::Bool(!b),
            (_, Val::Unknown) => Val::Unknown,
            _ => Val::Opaque,
        },
        Expr::Binary { op, lhs, rhs, .. } => {
            let l = eval(lhs, parse_expr);
            let r = eval(rhs, parse_expr);
            binary(op, l, r)
        }
        Expr::Error { .. } => Val::Opaque,
    }
}

pub fn eval_arg(a: &Arg, parse_expr: &dyn Fn(&str) -> Option<Expr>) -> Val {
    match a {
        Arg::Expr(e) => eval(e, parse_expr),
        Arg::Kw(k) => Val::Record(vec![(k.name.clone(), eval_arg(&k.value, parse_expr))]),
    }
}

fn binary(op: &str, l: Val, r: Val) -> Val {
    if matches!(l, Val::Unknown) || matches!(r, Val::Unknown) {
        return Val::Unknown;
    }
    match (op, &l, &r) {
        ("+", Val::Int(a), Val::Int(b)) => Val::Int(a + b),
        ("-", Val::Int(a), Val::Int(b)) => Val::Int(a - b),
        ("*", Val::Int(a), Val::Int(b)) => Val::Int(a * b),
        ("/", Val::Int(a), Val::Int(b)) if *b != 0 => Val::Int(a / b),
        ("%", Val::Int(a), Val::Int(b)) if *b != 0 => Val::Int(a % b),
        ("+", Val::Duration(a), Val::Duration(b)) => {
            Val::Duration(Duration::new(a.seconds + b.seconds))
        }
        ("-", Val::Duration(a), Val::Duration(b)) => {
            Val::Duration(Duration::new(a.seconds.saturating_sub(b.seconds)))
        }
        ("==", a, b) if a.kind().is_some() && b.kind().is_some() => Val::Bool(a == b),
        ("!=", a, b) if a.kind().is_some() && b.kind().is_some() => Val::Bool(a != b),
        ("<", Val::Int(a), Val::Int(b)) => Val::Bool(a < b),
        ("<=", Val::Int(a), Val::Int(b)) => Val::Bool(a <= b),
        (">", Val::Int(a), Val::Int(b)) => Val::Bool(a > b),
        (">=", Val::Int(a), Val::Int(b)) => Val::Bool(a >= b),
        ("<", Val::Duration(a), Val::Duration(b)) => Val::Bool(a < b),
        ("<=", Val::Duration(a), Val::Duration(b)) => Val::Bool(a <= b),
        (">", Val::Duration(a), Val::Duration(b)) => Val::Bool(a > b),
        (">=", Val::Duration(a), Val::Duration(b)) => Val::Bool(a >= b),
        ("and", Val::Bool(a), Val::Bool(b)) => Val::Bool(*a && *b),
        ("or", Val::Bool(a), Val::Bool(b)) => Val::Bool(*a || *b),
        _ => Val::Opaque,
    }
}
