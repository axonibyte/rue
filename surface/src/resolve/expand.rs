//! Expansion (docs/ROADMAP.md 6.4 "Modules" and "Clauses"): a plan clause
//! for one host into `rue_core::model::Plan`, every step's op template
//! expanded at its call with its parameters bound, bodies lowered to
//! primitives with every reference classified by its origin, footprints
//! to shapes by the one rule, gates to their tree. What the text leaves
//! implicit is decided here and stated in docs/LANGUAGE.md.

use std::collections::BTreeMap;

use rowan::TextRange;
use rue_core::body::{self as b, Body, FactRef, Part, Prim, Ref, Template, Value};
use rue_core::diagnostics::{nearest, Code, Diagnostic};
use rue_core::model::*;

use super::site::{self, Contract};
use super::value::{self, Val};
use super::{diag, span_of, Program};
use crate::ast::{Arg, Def, Expr, Lit, Pattern, Step, Stmt};

pub struct Context<'a> {
    pub program: &'a Program,
    pub contracts: &'a [Contract],
    pub site: &'a Site,
}

/// How a name bound at an op's call reaches the op's body.
#[derive(Debug, Clone)]
enum Bound {
    /// A literal: the request supplies it; the body sees a parameter.
    Literal(Val),
    /// A `repeat` variable or a controller probe's fact.
    Controller,
    /// An earlier step's output.
    Output {
        step: String,
        name: String,
        secret: bool,
    },
    /// Some other plan-level name: a parameter the request binds.
    Name,
}

/// What a body sees of the world around it.
#[derive(Debug, Clone, Default)]
struct Scope {
    /// `repeat` variables in scope.
    loop_vars: Vec<String>,
    /// Step aliases in scope with their outputs.
    aliases: BTreeMap<String, Vec<Output>>,
    /// Probe facts by name with their locus (target or controller).
    probes: BTreeMap<String, bool>,
}

/// The op parameters bound at one call.
type Bindings = BTreeMap<String, Bound>;

fn parse_expr(s: &str) -> Option<Expr> {
    crate::parser::parse_expr(s)
}

/// `%{name: "db-01"}` names the host a plan clause is for.
pub fn pattern_host_name(d: &Def) -> Option<String> {
    let p = d.pattern.as_ref()?;
    if let Expr::Record { entries, .. } = &p.expr {
        for e in entries {
            if e.name == "name" {
                if let Arg::Expr(Expr::Lit {
                    lit: Lit::Str(s), ..
                }) = &*e.value
                {
                    return Some(crate::ast::unquote(s));
                }
            }
        }
    }
    None
}

impl<'a> Context<'a> {
    fn src(&self, module: usize, range: TextRange) -> String {
        let s = &self.program.modules[module].src;
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        s[start.min(s.len())..end.min(s.len())].to_string()
    }

    fn err(&self, module: usize, range: TextRange, code: Code, message: String) -> Diagnostic {
        diag(
            code,
            Some(span_of(&self.program.modules[module], range)),
            message,
        )
    }

    // --- clause dispatch --------------------------------------------------

    /// Does a clause pattern match a host's contract? `None` with a
    /// diagnostic when the pattern names a fact the contract lacks (E0111).
    fn matches(
        &self,
        module: usize,
        pattern: Option<&Pattern>,
        c: &Contract,
        diags: &mut Vec<Diagnostic>,
    ) -> bool {
        let Some(p) = pattern else { return true };
        match &p.expr {
            Expr::Ref { path, .. } if path.len() == 1 && path[0] == "_" => true,
            Expr::Ref { .. } => true,
            Expr::Record { entries, .. } => {
                let mut ok = true;
                for e in entries {
                    let key = crate::ast::unquote(&e.name);
                    let actual: Vec<String> = match key.as_str() {
                        "name" => vec![c.name.clone()],
                        "os" => vec![c.os.clone()],
                        "address" => vec![c.address.clone()],
                        "roles" => c.roles.clone(),
                        "reach" => c.reach.clone(),
                        other => {
                            diags.push(self.err(
                                module,
                                e.range,
                                Code::E0111,
                                format!("clause pattern names `{other}`, which is not a host-contract fact (name, os, address, roles, reach)"),
                            ));
                            ok = false;
                            continue;
                        }
                    };
                    let wanted: Vec<String> = match &*e.value {
                        Arg::Expr(Expr::List { items, .. }) => items
                            .iter()
                            .filter_map(|a| match a {
                                Arg::Expr(x) => pat_text(x),
                                _ => None,
                            })
                            .collect(),
                        Arg::Expr(x) => pat_text(x).into_iter().collect(),
                        _ => Vec::new(),
                    };
                    let hit = match &*e.value {
                        // A list pattern against a scalar: any member; against
                        // a list fact: every member present.
                        Arg::Expr(Expr::List { .. }) if key == "roles" || key == "reach" => {
                            wanted.iter().all(|w| actual.contains(w))
                        }
                        Arg::Expr(Expr::List { .. }) => wanted.iter().any(|w| actual.contains(w)),
                        _ => wanted.iter().any(|w| actual.contains(w)),
                    };
                    if !hit {
                        ok = false;
                    }
                }
                ok
            }
            _ => true,
        }
    }

    /// The clause of `defs` that matches the host, or E0112 (no match).
    fn dispatch<'d>(
        &self,
        defs: &[(usize, &'d Def)],
        c: &Contract,
        what: &str,
        at: (usize, TextRange),
        diags: &mut Vec<Diagnostic>,
    ) -> Option<(usize, &'d Def, Contract)> {
        for (m, d) in defs {
            if self.matches(*m, d.pattern.as_ref(), c, diags) {
                return Some((*m, d, c.clone()));
            }
        }
        // An op whose clause declares a static host is dispatched on it.
        for (m, d) in defs {
            if let Some(h) = static_locus_host(d) {
                if let Some(hc) = self.contracts.iter().find(|x| x.name == h) {
                    if self.matches(*m, d.pattern.as_ref(), hc, diags) {
                        return Some((*m, d, hc.clone()));
                    }
                }
            }
        }
        diags.push(self.err(
            at.0,
            at.1,
            Code::E0112,
            format!("no clause of {what} matches host {} (os {})", c.name, c.os),
        ));
        None
    }

    // --- the plan ---------------------------------------------------------

    pub fn expand_plan(
        &self,
        clauses: &[(usize, &Def)],
        owner: &Contract,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Plan> {
        self.check_duplicates(clauses, diags);
        let (module, def, _) = self.dispatch(
            clauses,
            owner,
            &format!("plan {}", clauses[0].1.name),
            (clauses[0].0, clauses[0].1.name_range),
            diags,
        )?;
        let mut scope = Scope::default();
        for (_, p) in self.program.defs(module, "defprobe") {
            let controller = super::lines(&p.body).any(|l| {
                l.keyword == "locus"
                    && l.args.iter().any(|a| matches!(a, Arg::Expr(Expr::Lit { lit: Lit::Atom(x), .. }) if x == "controller"))
            });
            for l in super::lines(&p.body).filter(|l| l.keyword == "produces") {
                for a in &l.args {
                    if let Arg::Expr(Expr::Ref { path, .. }) = a {
                        scope.probes.insert(path.join("."), controller);
                    }
                }
            }
            scope.probes.entry(p.name.clone()).or_insert(controller);
        }
        for (alias, &m) in &self.program.modules[module].imports {
            for (_, p) in self.program.defs(m, "defprobe") {
                let controller = super::lines(&p.body).any(|l| {
                    l.keyword == "locus"
                        && l.args.iter().any(|a| matches!(a, Arg::Expr(Expr::Lit { lit: Lit::Atom(x), .. }) if x == "controller"))
                });
                scope
                    .probes
                    .entry(format!("{alias}.{}", p.name))
                    .or_insert(controller);
            }
        }
        let mut plan = Plan::new(&def.name, &owner.name, Vec::new());
        // Plan options: the keyword lines at the top of the body.
        for l in super::lines(&def.body) {
            match l.keyword.as_str() {
                "gate" => {
                    let expr = l.args.first().and_then(|a| match a {
                        Arg::Expr(e) => self.gate(module, e, &Bindings::new(), diags),
                        _ => None,
                    });
                    if let Some(expr) = expr {
                        plan.gate = Some(PlanGate {
                            expr,
                            window: kw_duration(&l.args, "window"),
                            allow_zero_human: kw_bool(&l.args, "allow_zero_human").unwrap_or(false),
                        });
                    }
                }
                "wane" => {
                    plan.wane = first_duration(&l.args);
                    plan.renew_within = kw_duration(&l.args, "renew_within");
                }
                "backstop" => {
                    let mut triggers = Vec::new();
                    if let Some(Arg::Expr(Expr::List { items, .. })) =
                        site::kw(&l.args, "trigger").map(|k| &*k.value)
                    {
                        let mut pending_heartbeat: Option<usize> = None;
                        for it in items {
                            if let Arg::Kw(k) = it {
                                let d = match &*k.value {
                                    Arg::Expr(Expr::Lit {
                                        lit: Lit::Duration(s),
                                        ..
                                    }) => Duration::new(*s),
                                    _ => continue,
                                };
                                match k.name.as_str() {
                                    "after" => triggers.push(Trigger::After(d)),
                                    "unless_confirmed" => {
                                        triggers.push(Trigger::UnlessConfirmed(d))
                                    }
                                    "unless_heartbeat" => {
                                        pending_heartbeat = Some(triggers.len());
                                        triggers.push(Trigger::UnlessHeartbeat {
                                            deadline: d,
                                            interval: None,
                                        });
                                    }
                                    "interval" => {
                                        if let Some(i) = pending_heartbeat {
                                            if let Trigger::UnlessHeartbeat { interval, .. } =
                                                &mut triggers[i]
                                            {
                                                *interval = Some(d);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    let arm_before = match site::kw(&l.args, "arm_before").map(|k| &*k.value) {
                        Some(Arg::Expr(Expr::Lit {
                            lit: Lit::Int(n), ..
                        })) => *n as u32,
                        _ => 1,
                    };
                    plan.backstop = Some(Backstop {
                        triggers,
                        arm_before,
                    });
                }
                "mode" => {
                    if let Some(a) = first_atom(&l.args) {
                        plan.mode = if a == "auto" {
                            Mode::Auto
                        } else {
                            Mode::Manual
                        };
                    }
                }
                "strictness" => {
                    if let Some(a) = first_atom(&l.args) {
                        plan.strictness = if a == "warn" {
                            Strictness::Warn
                        } else {
                            Strictness::Strict
                        };
                    }
                }
                "exclusivity" => plan.exclusivity = first_atom(&l.args),
                "fires_by_construction" => {
                    plan.fires_by_construction = first_bool(&l.args).unwrap_or(false)
                }
                "require" => {
                    if let Some(Arg::Expr(Expr::Lit {
                        lit: Lit::Atom(a), ..
                    })) = site::kw(&l.args, "journal").map(|k| &*k.value)
                    {
                        plan.require_journal = Some(if a == "signed" {
                            JournalRequirement::Signed
                        } else {
                            JournalRequirement::Chained
                        });
                    }
                }
                _ => {}
            }
        }
        plan.body = self.expand_items(module, &def.body, owner, &mut scope, diags);
        Some(plan)
    }

    /// Two clauses of one name with the same pattern are indistinguishable (E0103).
    fn check_duplicates(&self, clauses: &[(usize, &Def)], diags: &mut Vec<Diagnostic>) {
        for (i, (m, d)) in clauses.iter().enumerate() {
            let pat = d
                .pattern
                .as_ref()
                .map(|p| self.src(*m, p.range))
                .unwrap_or_default();
            for (m2, d2) in clauses.iter().skip(i + 1) {
                let pat2 = d2
                    .pattern
                    .as_ref()
                    .map(|p| self.src(*m2, p.range))
                    .unwrap_or_default();
                if *m == *m2 && pat == pat2 {
                    diags.push(self.err(
                        *m2,
                        d2.name_range,
                        Code::E0103,
                        format!("{} is defined twice with the same clause pattern", d.name),
                    ));
                }
            }
        }
    }

    fn expand_items(
        &self,
        module: usize,
        body: &[Stmt],
        owner: &Contract,
        scope: &mut Scope,
        diags: &mut Vec<Diagnostic>,
    ) -> Vec<Item> {
        let mut items = Vec::new();
        for st in body {
            match st {
                Stmt::Line(l) => {
                    // Plan options are read by expand_plan; a bare guard line
                    // inside a preflight is read there; anything else here is
                    // a step written without parentheses.
                    if !PLAN_OPTS.contains(&l.keyword.as_str()) {
                        diags.push(self.err(
                            module,
                            l.range,
                            Code::E0101,
                            format!(
                                "`{}` is not an item; a step is a call, `{}()`",
                                l.keyword, l.keyword
                            ),
                        ));
                    }
                }
                Stmt::Step(s) => {
                    if let Some(it) = self.expand_step(module, s, owner, scope, false, diags) {
                        items.push(it);
                    }
                }
                Stmt::Pipeline(steps) => {
                    for s in steps {
                        if let Some(it) = self.expand_step(module, s, owner, scope, false, diags) {
                            items.push(it);
                        }
                    }
                }
                Stmt::Knell(s) => {
                    if let Some(it) = self.expand_step(module, s, owner, scope, true, diags) {
                        items.push(it);
                    }
                }
                Stmt::Block(bl) if bl.keyword == "par" => {
                    let children = self.expand_items(module, &bl.body, owner, scope, diags);
                    items.push(Item::Par { children });
                }
                Stmt::Block(bl) if bl.keyword == "preflight" => {
                    let mut guards = Vec::new();
                    for st in &bl.body {
                        match st {
                            Stmt::Line(l) => guards.push(Guard::new(&l.keyword, Tri::Yes)),
                            Stmt::Step(s) => {
                                if let Expr::Call { path, .. } = &s.call {
                                    guards.push(Guard::new(&path.join("."), Tri::Yes));
                                }
                            }
                            _ => {}
                        }
                    }
                    items.push(Item::Preflight { guards });
                }
                Stmt::Block(bl) => {
                    diags.push(self.err(
                        module,
                        bl.range,
                        Code::E0101,
                        format!("`{}` is not an item", bl.keyword),
                    ));
                }
                Stmt::Slot { name, .. } => items.push(Item::Slot { name: name.clone() }),
                Stmt::Observe { call, alias, range } => {
                    let probe = match call {
                        Expr::Call { path, .. } => self.probe_name(module, path, *range, diags),
                        _ => None,
                    };
                    if let (Some(probe), Some(alias)) = (probe, alias) {
                        scope.aliases.insert(alias.clone(), Vec::new());
                        items.push(Item::Observe {
                            probe,
                            alias: alias.clone(),
                        });
                    }
                }
                Stmt::Assert { args, range } => {
                    if let Some(guard) = self.guard_of(module, args, *range, diags) {
                        items.push(Item::Assert {
                            guard,
                            window: kw_duration(args, "window"),
                            on_lapse: on_lapse(args),
                        });
                    }
                }
                Stmt::Repeat {
                    args,
                    var,
                    body,
                    range,
                } => {
                    let var = var.clone().unwrap_or_else(|| "i".to_string());
                    let form = if let Some(over) = site::kw(args, "over") {
                        let list = match &*over.value {
                            Arg::Expr(Expr::Ref { path, .. }) => path.join("."),
                            Arg::Expr(Expr::List { items, .. }) => {
                                // A literal list is set-valued iff its members are distinct.
                                let texts: Vec<String> =
                                    items.iter().map(|a| self.src(module, a.range())).collect();
                                let mut sorted = texts.clone();
                                sorted.sort();
                                sorted.dedup();
                                if sorted.len() != texts.len() {
                                    diags.push(self.err(module, over.range, Code::E0113, "repeat over: a list with a repeated member is not set-valued".into()));
                                }
                                self.src(module, over.value.range())
                            }
                            Arg::Expr(e) => {
                                diags.push(self.err(
                                    module,
                                    e.range(),
                                    Code::E0113,
                                    "repeat over: needs a set-valued fact or a list".into(),
                                ));
                                self.src(module, e.range())
                            }
                            Arg::Kw(k) => self.src(module, k.range),
                        };
                        let max = match site::kw(args, "max").map(|k| &*k.value) {
                            Some(Arg::Expr(Expr::Lit {
                                lit: Lit::Int(n), ..
                            })) => *n as u32,
                            _ => {
                                diags.push(self.err(module, *range, Code::E0106, "repeat over: without max: is unbounded; give the literal cap the bounded form needs".into()));
                                0
                            }
                        };
                        RepeatForm::Over {
                            list,
                            max,
                            set_valued: true,
                        }
                    } else {
                        match args.first() {
                            Some(Arg::Expr(Expr::Lit {
                                lit: Lit::Int(n), ..
                            })) => RepeatForm::Count(*n as u32),
                            _ => {
                                diags.push(self.err(
                                    module,
                                    *range,
                                    Code::E0106,
                                    "repeat needs a literal count or over: with max:".into(),
                                ));
                                RepeatForm::Count(0)
                            }
                        }
                    };
                    scope.loop_vars.push(var.clone());
                    let body = self.expand_items(module, body, owner, scope, diags);
                    scope.loop_vars.pop();
                    items.push(Item::Repeat { form, var, body });
                }
                Stmt::When {
                    args,
                    then_,
                    else_,
                    range,
                } => {
                    if let Some(guard) = self.guard_of(module, args, *range, diags) {
                        let then_ = self.expand_items(module, then_, owner, scope, diags);
                        let else_ = else_
                            .as_ref()
                            .map(|e| self.expand_items(module, e, owner, scope, diags))
                            .unwrap_or_default();
                        items.push(Item::When {
                            guard,
                            window: kw_duration(args, "window"),
                            on_lapse: on_lapse(args),
                            then_,
                            else_,
                        });
                    }
                }
                Stmt::Contribution { range, .. } => {
                    diags.push(self.err(
                        module,
                        *range,
                        Code::E0101,
                        "a slot contribution belongs in a defrole".into(),
                    ));
                }
                Stmt::Def(d) => diags.push(self.err(
                    module,
                    d.range,
                    Code::E0101,
                    "a definition belongs at the top of the file".into(),
                )),
                Stmt::Import(i) => diags.push(self.err(
                    module,
                    i.range,
                    Code::E0101,
                    "an import belongs at the top of the file".into(),
                )),
            }
        }
        items
    }

    fn probe_name(
        &self,
        module: usize,
        path: &[String],
        range: TextRange,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<String> {
        let defs = self.program.lookup(module, "defprobe", path);
        if defs.is_empty() {
            let mut d = self.err(
                module,
                range,
                Code::E0102,
                format!("unknown probe {}", path.join(".")),
            );
            let names = self.program.visible_names(module, "defprobe");
            d.nearest = nearest(&path.join("."), names.iter().map(String::as_str));
            diags.push(d);
            return None;
        }
        Some(defs[0].1.name.clone())
    }

    /// A guard from an assert's or when's arguments: `[force: never,] expr`.
    fn guard_of(
        &self,
        module: usize,
        args: &[Arg],
        range: TextRange,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Guard> {
        let force_never = matches!(
            site::kw(args, "force").map(|k| &*k.value),
            Some(Arg::Expr(Expr::Ref { path, .. })) if path == &["never".to_string()]
        );
        let expr = args.iter().find_map(|a| match a {
            Arg::Expr(e) => Some(e),
            _ => None,
        });
        let Some(e) = expr else {
            diags.push(self.err(module, range, Code::E0101, "a guard is missing".into()));
            return None;
        };
        self.check_unknown_compare(module, e, diags);
        let name = match e {
            Expr::Ref { path, .. } => path.join("."),
            Expr::Call { path, .. } => path.join("."),
            other => self.src(module, other.range()),
        };
        Some(Guard {
            name,
            value: Tri::Yes,
            force_never,
        })
    }

    /// E0108: a comparison against `:unknown`.
    fn check_unknown_compare(&self, module: usize, e: &Expr, diags: &mut Vec<Diagnostic>) {
        match e {
            Expr::Binary {
                op,
                lhs,
                rhs,
                range,
            } => {
                let is_unknown =
                    |x: &Expr| matches!(x, Expr::Lit { lit: Lit::Atom(a), .. } if a == "unknown");
                if (op == "==" || op == "!=") && (is_unknown(lhs) || is_unknown(rhs)) {
                    diags.push(self.err(
                        module,
                        *range,
                        Code::E0108,
                        "comparison against :unknown; ask defined?() or unknown?() instead".into(),
                    ));
                }
                self.check_unknown_compare(module, lhs, diags);
                self.check_unknown_compare(module, rhs, diags);
            }
            Expr::Unary { expr, .. } | Expr::Paren { expr, .. } => {
                self.check_unknown_compare(module, expr, diags)
            }
            Expr::Call { args, .. } | Expr::List { items: args, .. } => {
                for a in args {
                    if let Arg::Expr(x) = a {
                        self.check_unknown_compare(module, x, diags);
                    }
                }
            }
            _ => {}
        }
    }

    // --- steps and ops ----------------------------------------------------

    fn expand_step(
        &self,
        module: usize,
        s: &Step,
        owner: &Contract,
        scope: &mut Scope,
        knell: bool,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Item> {
        let Expr::Call { path, args, range } = &s.call else {
            diags.push(self.err(module, s.range, Code::E0101, "a step is a call".into()));
            return None;
        };
        if path.len() == 1 && args.is_empty() && !knell {
            match path[0].as_str() {
                "confirm" => return Some(Item::Confirm),
                "commit" => return Some(Item::Commit),
                _ => {}
            }
        }
        // The call's keyword arguments bind the op's parameters.
        let mut bindings: Bindings = BTreeMap::new();
        let mut rendered_args = Vec::new();
        for a in args {
            if let Arg::Kw(k) = a {
                let bound = self.bind(module, &k.value, scope, diags);
                bindings.insert(k.name.clone(), bound);
                rendered_args.push(format!("{}: {}", k.name, self.render_arg(module, &k.value)));
            }
        }
        let op = self.expand_op(
            CallSite {
                module,
                path,
                range: *range,
            },
            &bindings,
            owner,
            scope,
            diags,
        )?;
        let mut st = StepI::new(op);
        st.args = rendered_args;
        st.alias = s.alias.clone();
        for k in &s.kws {
            match k.name.as_str() {
                "gate" => {
                    if let Arg::Expr(e) = &*k.value {
                        st.gate = self.gate(module, e, &bindings, diags);
                    }
                }
                "window" => st.window = duration_of(&k.value),
                "on_lapse" => {
                    st.on_lapse = if atom_of(&k.value).as_deref() == Some("hold") {
                        OnLapse::Hold
                    } else {
                        OnLapse::Revert
                    }
                }
                "force" => {
                    if let Arg::Expr(Expr::List { items, .. }) = &*k.value {
                        for it in items {
                            match it {
                                Arg::Expr(Expr::Ref { path, .. })
                                    if path == &["unknown".to_string()] =>
                                {
                                    st.force.push(ForceName::Unknown)
                                }
                                Arg::Expr(Expr::Ref { path, .. })
                                    if path == &["drift".to_string()] =>
                                {
                                    st.force.push(ForceName::Drift)
                                }
                                Arg::Expr(Expr::Lit {
                                    lit: Lit::Atom(a), ..
                                }) => st.force.push(ForceName::Guard(a.clone())),
                                Arg::Expr(Expr::Ref { path, .. }) => {
                                    st.force.push(ForceName::Guard(path.join(".")))
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(alias) = &s.alias {
            scope.aliases.insert(alias.clone(), st.op.outputs.clone());
        }
        Some(if knell {
            Item::Knell(st)
        } else {
            Item::Step(st)
        })
    }

    /// How a call-site value reaches the body (E0110 for an output not
    /// yet in scope).
    fn bind(
        &self,
        module: usize,
        value: &Arg,
        scope: &Scope,
        diags: &mut Vec<Diagnostic>,
    ) -> Bound {
        match value {
            Arg::Expr(Expr::Ref { path, range }) => match path.as_slice() {
                [n] if scope.loop_vars.contains(n) => Bound::Controller,
                [n] if scope.probes.get(n) == Some(&true) => Bound::Controller,
                [alias, name] if scope.aliases.contains_key(alias) => {
                    let secret = scope.aliases[alias]
                        .iter()
                        .any(|o| &o.name == name && o.secret);
                    Bound::Output {
                        step: alias.clone(),
                        name: name.clone(),
                        secret,
                    }
                }
                [alias, _] if self.program.modules[module].imports.contains_key(alias) => {
                    Bound::Name
                }
                [alias, name] if alias != "host" => {
                    diags.push(self.err(
                        module,
                        *range,
                        Code::E0110,
                        format!("{alias}.{name} is read before a step named {alias} ran"),
                    ));
                    Bound::Name
                }
                _ => Bound::Name,
            },
            Arg::Expr(e) => Bound::Literal(value::eval(e, &parse_expr)),
            Arg::Kw(_) => Bound::Name,
        }
    }

    fn render_arg(&self, module: usize, value: &Arg) -> String {
        match value {
            Arg::Expr(Expr::Lit {
                lit: Lit::Str(s), ..
            }) => crate::ast::unquote(s),
            other => self.src(module, other.range()),
        }
    }

    fn expand_op(
        &self,
        call: CallSite<'_>,
        bindings: &Bindings,
        owner: &Contract,
        scope: &Scope,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Op> {
        let CallSite {
            module,
            path,
            range,
        } = call;
        let defs = self.program.lookup(module, "defop", path);
        if defs.is_empty() {
            let mut d = self.err(
                module,
                range,
                Code::E0102,
                format!("unknown op {}", path.join(".")),
            );
            let names = self.program.visible_names(module, "defop");
            d.nearest = nearest(&path.join("."), names.iter().map(String::as_str));
            diags.push(d);
            return None;
        }
        self.check_duplicates(&defs, diags);
        let (m, def, _host) = self.dispatch(
            &defs,
            owner,
            &format!("op {}", path.join(".")),
            (module, range),
            diags,
        )?;
        // Declared parameters with defaults fill what the call left unbound.
        let mut bindings = bindings.clone();
        for p in &def.params {
            if bindings.contains_key(&p.name) {
                continue;
            }
            match &*p.value {
                Arg::Expr(Expr::Ref { path, .. }) if path.len() == 1 && path[0] == p.name => {
                    diags.push(self.err(
                        module,
                        range,
                        Code::E0102,
                        format!("op {} needs its parameter {}", def.name, p.name),
                    ));
                }
                Arg::Expr(e) => {
                    bindings.insert(p.name.clone(), Bound::Literal(value::eval(e, &parse_expr)));
                }
                Arg::Kw(_) => {}
            }
        }
        let cx = OpCx {
            module: m,
            def,
            bindings: &bindings,
            scope,
        };
        let mut op = Op::new(&def.name, Vec::new());
        let mut has_undo_line = false;
        for l in super::lines(&def.body) {
            match l.keyword.as_str() {
                "footprint" => {
                    for a in &l.args {
                        if let Arg::Kw(k) = a {
                            let kind = match k.name.as_str() {
                                "owned" => Kind::Owned,
                                "region" => Kind::Region,
                                "modified" => Kind::Modified,
                                "derived" => Kind::Derived,
                                "append_only" => Kind::AppendOnly,
                                "held" => Kind::Held,
                                other => {
                                    diags.push(self.err(
                                        m,
                                        k.range,
                                        Code::E0101,
                                        format!("unknown footprint kind {other}"),
                                    ));
                                    continue;
                                }
                            };
                            if let Some((shape, anchor)) =
                                self.shape(&cx, &k.value, kind == Kind::Derived, diags)
                            {
                                op.footprint.push(FootprintEntry {
                                    kind,
                                    shape,
                                    instance: None,
                                    anchor,
                                });
                            }
                        }
                    }
                }
                "reach" => {
                    for a in &l.args {
                        if let Arg::Expr(Expr::Call { path, .. }) = a {
                            op.reach.push(path.join("."));
                        }
                    }
                }
                "pre" => op.pre = self.guards(&cx, &l.args, diags),
                "post" => op.post = self.guards(&cx, &l.args, diags),
                "do" => op.do_ = self.body_of(&cx, &l.args, diags),
                "undo" => {
                    has_undo_line = true;
                    op.undo = match l.args.first() {
                        Some(Arg::Expr(Expr::Lit {
                            lit: Lit::Atom(a), ..
                        })) if a == "restore" => Undo::Restore,
                        Some(Arg::Kw(k)) if k.name == "compensate" => Undo::Compensate {
                            body: self.body_of(&cx, std::slice::from_ref(&k.value), diags),
                            undo_pre: Vec::new(),
                        },
                        _ => Undo::Computed {
                            body: self.body_of(&cx, &l.args, diags),
                            undo_pre: Vec::new(),
                        },
                    };
                }
                "undo_pre" => {
                    let pre: Vec<String> = l
                        .args
                        .iter()
                        .filter_map(|a| self.shape(&cx, a, false, diags).map(|(s, _)| s))
                        .collect();
                    match &mut op.undo {
                        Undo::Computed { undo_pre, .. } | Undo::Compensate { undo_pre, .. } => {
                            *undo_pre = pre
                        }
                        _ => {}
                    }
                }
                "undo_locus" => {
                    op.undo_locus = match first_atom(&l.args).as_deref() {
                        Some("target") => UndoLocus::Target,
                        Some("none") => UndoLocus::NoLocus,
                        _ => UndoLocus::Controller,
                    }
                }
                "refusal" => op.refusal = self.refusal(&cx, &l.args, diags),
                "drift" => {
                    let v = l.args.first().map(|a| self.value_val(&cx, a));
                    op.drift = match v.as_ref().and_then(|v| v.as_atom()) {
                        Some("clobber") => Some(Drift::Clobber),
                        Some("defer") => Some(Drift::Defer),
                        _ => None,
                    };
                }
                "outputs" => {
                    let mut current: Option<Output> = None;
                    for a in &l.args {
                        match a {
                            Arg::Expr(Expr::Ref { path, .. }) => {
                                if let Some(o) = current.take() {
                                    op.outputs.push(o);
                                }
                                current = Some(Output {
                                    name: path.join("."),
                                    secret: false,
                                });
                            }
                            Arg::Kw(k) if k.name == "secret" => {
                                if let Some(o) = &mut current {
                                    o.secret = matches!(
                                        &*k.value,
                                        Arg::Expr(Expr::Lit {
                                            lit: Lit::Bool(true),
                                            ..
                                        })
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    if let Some(o) = current {
                        op.outputs.push(o);
                    }
                }
                "exclusivity" => op.exclusivity = first_atom(&l.args),
                "locus" => {
                    op.locus = match l.args.first() {
                        Some(Arg::Expr(Expr::Lit {
                            lit: Lit::Atom(a), ..
                        })) if a == "controller" => Locus::Controller,
                        Some(Arg::Expr(Expr::Lit {
                            lit: Lit::Atom(a), ..
                        })) if a == "target" => Locus::Target,
                        Some(Arg::Expr(Expr::Call { path, args, .. }))
                            if path == &["host".to_string()] =>
                        {
                            match args.first() {
                                Some(Arg::Expr(Expr::Lit {
                                    lit: Lit::Str(s), ..
                                })) => Locus::Host(HostRef::Static(crate::ast::unquote(s))),
                                Some(Arg::Expr(Expr::Ref { path, .. })) => {
                                    Locus::Host(HostRef::Bound(path.join(".")))
                                }
                                _ => Locus::Target,
                            }
                        }
                        _ => Locus::Target,
                    }
                }
                "handoff_done" => {
                    op.handoff_done = l.args.first().and_then(|a| match a {
                        Arg::Expr(Expr::Call { path, .. }) => Some(path.join(".")),
                        Arg::Expr(Expr::Ref { path, .. }) => Some(path.join(".")),
                        _ => None,
                    })
                }
                "suspend" => op.suspend = Some(self.body_of(&cx, &l.args, diags)),
                "reestablish" => op.reestablish = Some(self.body_of(&cx, &l.args, diags)),
                other => diags.push(self.err(
                    m,
                    l.range,
                    Code::E0101,
                    format!("`{other}` is not an op line"),
                )),
            }
        }
        if !has_undo_line {
            op.undo = Undo::NoUndo;
        }
        op.undo_idempotent = idempotent(&cx, &def.body, &op.undo);
        Some(op)
    }

    fn guards(&self, cx: &OpCx, args: &[Arg], _diags: &mut Vec<Diagnostic>) -> Vec<Guard> {
        args.iter()
            .filter_map(|a| match a {
                Arg::Expr(Expr::Ref { path, .. }) => Some(Guard::new(&path.join("."), Tri::Yes)),
                Arg::Expr(Expr::Call { path, .. }) => Some(Guard::new(&path.join("."), Tri::Yes)),
                Arg::Expr(e) => Some(Guard::new(&self.src(cx.module, e.range()), Tri::Yes)),
                Arg::Kw(_) => None,
            })
            .collect()
    }

    fn refusal(&self, cx: &OpCx, args: &[Arg], diags: &mut Vec<Diagnostic>) -> Refusal {
        match args.first() {
            Some(Arg::Expr(Expr::Lit {
                lit: Lit::Atom(a), ..
            })) if a == "hold" => Refusal::Hold {
                via: site::kw(args, "via").and_then(|k| atom_of(&k.value)),
            },
            Some(Arg::Expr(Expr::Ref { path, .. })) if path == &["knell".to_string()] => {
                let guard = site::kw(args, "guard").map(|k| match &*k.value {
                    Arg::Expr(Expr::Ref { path, .. }) => Guard::new(&path.join("."), Tri::Yes),
                    other => Guard::new(&self.src(cx.module, other.range()), Tri::Yes),
                });
                let cost = match site::kw(args, "cost").map(|k| &*k.value) {
                    Some(Arg::Expr(Expr::Call { path, .. })) => Cost::Probe(path.join(".")),
                    Some(Arg::Expr(Expr::Lit {
                        lit: Lit::Atom(a), ..
                    })) if a == "none" => Cost::NoCost(
                        site::kw(args, "reason")
                            .and_then(|k| str_of(&k.value))
                            .unwrap_or_default(),
                    ),
                    _ => {
                        diags.push(self.err(
                            cx.module,
                            args.first().map(|a| a.range()).unwrap_or_default(),
                            Code::E0204,
                            format!(
                                "knell {} needs cost: a probe or :none with a reason",
                                cx.def.name
                            ),
                        ));
                        Cost::NoCost(String::new())
                    }
                };
                let ack = match site::kw(args, "ack").map(|k| &*k.value) {
                    None => Ack::Gate(GateExpr::Single(Factor::Humans { weight: 1 })),
                    Some(v) => self.ack_of(cx, v, args, diags),
                };
                Refusal::Knell { guard, cost, ack }
            }
            _ => Refusal::Revert,
        }
    }

    /// An ack: a gate expression, `:none` with a `reason:`, or a parameter
    /// bound at the call to either.
    fn ack_of(&self, cx: &OpCx, v: &Arg, siblings: &[Arg], diags: &mut Vec<Diagnostic>) -> Ack {
        match v {
            Arg::Expr(Expr::Lit {
                lit: Lit::Atom(a), ..
            }) if a == "none" => Ack::NoAck(
                site::kw(siblings, "reason")
                    .and_then(|k| str_of(&k.value))
                    .unwrap_or_default(),
            ),
            Arg::Expr(Expr::Ref { path, .. })
                if path.len() == 1 && cx.bindings.contains_key(&path[0]) =>
            {
                match &cx.bindings[&path[0]] {
                    Bound::Literal(Val::Atom(a)) if a == "none" => Ack::NoAck(
                        cx.bindings
                            .get("reason")
                            .and_then(|b| match b {
                                Bound::Literal(Val::Str(s)) => Some(s.clone()),
                                _ => None,
                            })
                            .unwrap_or_default(),
                    ),
                    Bound::Literal(Val::Call(path, args)) => {
                        let e = Expr::Call {
                            range: TextRange::default(),
                            path: path.clone(),
                            args: args.clone(),
                        };
                        self.gate(cx.module, &e, cx.bindings, diags)
                            .map(Ack::Gate)
                            .unwrap_or(Ack::NoAck(String::new()))
                    }
                    _ => Ack::Gate(GateExpr::Single(Factor::Humans { weight: 1 })),
                }
            }
            Arg::Expr(e) => self
                .gate(cx.module, e, cx.bindings, diags)
                .map(Ack::Gate)
                .unwrap_or(Ack::NoAck(String::new())),
            Arg::Kw(_) => Ack::NoAck(String::new()),
        }
    }

    /// A gate expression (5.11).
    fn gate(
        &self,
        module: usize,
        e: &Expr,
        bindings: &Bindings,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<GateExpr> {
        match e {
            Expr::Call { path, args, range } => {
                let weight = site::kw(args, "weight")
                    .and_then(|k| int_of(&k.value))
                    .unwrap_or(1) as u32;
                match path.join(".").as_str() {
                    "auth" => Some(GateExpr::Single(Factor::Auth {
                        id: args
                            .iter()
                            .find_map(|a| match a {
                                Arg::Expr(x) => atom_of(&Arg::Expr(x.clone())),
                                _ => None,
                            })
                            .unwrap_or_default(),
                        weight,
                    })),
                    "humans" => Some(GateExpr::Single(Factor::Humans { weight })),
                    "wait" => Some(GateExpr::Single(Factor::Wait {
                        duration: args
                            .iter()
                            .find_map(duration_of)
                            .unwrap_or(Duration::new(0)),
                        weight,
                    })),
                    "group" => {
                        let inner = args.iter().find_map(|a| match a {
                            Arg::Expr(x) => self.gate(module, x, bindings, diags),
                            _ => None,
                        })?;
                        Some(GateExpr::Single(Factor::Group {
                            expr: Box::new(inner),
                            weight,
                        }))
                    }
                    "thresh" => {
                        let n = args.first().and_then(int_of).unwrap_or(1) as u32;
                        let mut factors = Vec::new();
                        for a in args.iter().skip(1) {
                            if let Arg::Expr(x) = a {
                                if let Some(GateExpr::Single(f)) =
                                    self.gate(module, x, bindings, diags)
                                {
                                    factors.push(f);
                                } else if let Some(g) = self.gate(module, x, bindings, diags) {
                                    factors.push(Factor::Group {
                                        expr: Box::new(g),
                                        weight: 1,
                                    });
                                }
                            }
                        }
                        Some(GateExpr::Thresh { n, factors })
                    }
                    other => {
                        diags.push(self.err(
                            module,
                            *range,
                            Code::E0102,
                            format!("unknown gate factor {other}"),
                        ));
                        None
                    }
                }
            }
            Expr::Ref { path, .. } if path.len() == 1 => match bindings.get(&path[0]) {
                Some(Bound::Literal(Val::Call(p, a))) => {
                    let e = Expr::Call {
                        range: TextRange::default(),
                        path: p.clone(),
                        args: a.clone(),
                    };
                    self.gate(module, &e, bindings, diags)
                }
                _ => None,
            },
            _ => None,
        }
    }

    // --- bodies and values ------------------------------------------------

    fn body_of(&self, cx: &OpCx, args: &[Arg], diags: &mut Vec<Diagnostic>) -> Body {
        let mut body = Vec::new();
        for a in args {
            match a {
                Arg::Expr(Expr::List { items, .. }) => {
                    for it in items {
                        if let Some(p) = self.prim(cx, it, diags) {
                            body.push(p);
                        }
                    }
                }
                other => {
                    if let Some(p) = self.prim(cx, other, diags) {
                        body.push(p);
                    }
                }
            }
        }
        body
    }

    fn prim(&self, cx: &OpCx, a: &Arg, diags: &mut Vec<Diagnostic>) -> Option<Prim> {
        let Arg::Expr(Expr::Call { path, args, range }) = a else {
            diags.push(self.err(
                cx.module,
                a.range(),
                Code::E0101,
                "a body is a primitive call or a list of them".into(),
            ));
            return None;
        };
        let positional: Vec<&Expr> = args
            .iter()
            .filter_map(|x| match x {
                Arg::Expr(e) => Some(e),
                _ => None,
            })
            .collect();
        let kwv = |name: &str| site::kw(args, name).map(|k| &*k.value);
        match path.join(".").as_str() {
            "run" => {
                let cmd = match positional.first() {
                    Some(Expr::Lit {
                        lit: Lit::Str(s), ..
                    }) => self.template(cx, s, diags),
                    _ => {
                        diags.push(self.err(
                            cx.module,
                            *range,
                            Code::E0101,
                            "run takes a string".into(),
                        ));
                        return None;
                    }
                };
                let mut env = Vec::new();
                if let Some(Arg::Expr(Expr::Record { entries, .. })) = kwv("env") {
                    for e in entries {
                        env.push(b::EnvVar {
                            name: crate::ast::unquote(&e.name),
                            value: self.value(cx, &e.value, diags),
                        });
                    }
                }
                let stdin = kwv("stdin").map(|v| self.value(cx, v, diags));
                Some(Prim::Run(b::Run { cmd, env, stdin }))
            }
            "write" => Some(b::write(
                self.fact_ref(cx, positional.first()?, diags)?,
                self.value(cx, kwv("content")?, diags),
            )),
            "remove" => Some(b::remove(self.fact_ref(cx, positional.first()?, diags)?)),
            "append" => Some(b::append(
                self.fact_ref(cx, positional.first()?, diags)?,
                self.value(cx, kwv("line")?, diags),
            )),
            "region_set" => Some(b::region_set(
                self.fact_ref(cx, positional.first()?, diags)?,
                self.value(cx, kwv("content")?, diags),
            )),
            "region_clear" => Some(b::region_clear(self.fact_ref(
                cx,
                positional.first()?,
                diags,
            )?)),
            "stage" => {
                let name = match positional.first() {
                    Some(Expr::Ref { path, .. }) => path.join("."),
                    Some(e) => self.src(cx.module, e.range()),
                    None => String::new(),
                };
                let mode = kwv("mode").and_then(int_of).unwrap_or(0o600) as u32;
                Some(b::stage(
                    &name,
                    self.value(cx, kwv("content")?, diags),
                    mode,
                ))
            }
            "hook" => {
                let name = positional
                    .first()
                    .and_then(|e| match e {
                        Expr::Lit {
                            lit: Lit::Atom(a), ..
                        } => Some(a.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let mut kws = Vec::new();
                for x in args {
                    if let Arg::Kw(k) = x {
                        if k.name == "idempotent" {
                            continue;
                        }
                        kws.push(b::KwArg {
                            name: k.name.clone(),
                            value: self.value(cx, &k.value, diags),
                        });
                    }
                }
                Some(Prim::Hook(b::Hook { name, args: kws }))
            }
            "install" | "release" => {
                let name = positional
                    .first()
                    .and_then(|e| match e {
                        Expr::Lit {
                            lit: Lit::Atom(a), ..
                        } => Some(a.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                Some(if path[0] == "install" {
                    b::install(&name)
                } else {
                    b::release(&name)
                })
            }
            other => {
                let mut d = self.err(
                    cx.module,
                    *range,
                    Code::E0102,
                    format!("unknown primitive {other}"),
                );
                d.nearest = nearest(other, PRIMS.iter().copied());
                diags.push(d);
                None
            }
        }
    }

    /// A string as a template: literal parts and classified references.
    fn template(&self, cx: &OpCx, raw: &str, diags: &mut Vec<Diagnostic>) -> Template {
        value::string_parts(raw, &parse_expr)
            .into_iter()
            .map(|p| match p {
                value::Part::Lit(s) => Part::Lit(s),
                value::Part::Expr(e) => match self.classify_expr(cx, &e, diags) {
                    Some(r) => Part::Ref(r),
                    None => Part::Lit(self.src(cx.module, e.range())),
                },
            })
            .collect()
    }

    fn value(&self, cx: &OpCx, a: &Arg, diags: &mut Vec<Diagnostic>) -> Value {
        match a {
            Arg::Expr(Expr::Lit {
                lit: Lit::Str(s), ..
            }) => {
                let parts = self.template(cx, s, diags);
                if parts.iter().all(|p| matches!(p, Part::Lit(_))) {
                    Value::Lit(
                        parts
                            .iter()
                            .map(|p| match p {
                                Part::Lit(s) => s.as_str(),
                                _ => "",
                            })
                            .collect(),
                    )
                } else {
                    Value::Template(parts)
                }
            }
            Arg::Expr(e) => match self.classify_expr(cx, e, diags) {
                Some(r) => Value::Ref(r),
                None => Value::Lit(self.src(cx.module, e.range())),
            },
            Arg::Kw(k) => Value::Lit(self.src(cx.module, k.range)),
        }
    }

    fn value_val(&self, cx: &OpCx, a: &Arg) -> Val {
        match a {
            Arg::Expr(Expr::Ref { path, .. }) if path.len() == 1 => match cx.bindings.get(&path[0])
            {
                Some(Bound::Literal(v)) => v.clone(),
                _ => Val::Ref(path.clone()),
            },
            other => value::eval_arg(other, &parse_expr),
        }
    }

    /// The origin of a reference in a body (docs/LANGUAGE.md): a host
    /// field, a secret, an output, a loop variable or controller fact
    /// bound at the call, a target probe's fact, else a parameter.
    fn classify_expr(&self, cx: &OpCx, e: &Expr, diags: &mut Vec<Diagnostic>) -> Option<Ref> {
        match e {
            Expr::Ref { path, .. } => Some(self.classify(cx, path, diags)),
            Expr::Call { path, args, range } if path == &["secret".to_string()] => {
                let name = args.first().and_then(|a| match a {
                    Arg::Expr(Expr::Lit {
                        lit: Lit::Atom(x), ..
                    }) => Some(x.clone()),
                    _ => None,
                });
                match name {
                    Some(n) => Some(b::secret(&n)),
                    None => {
                        diags.push(self.err(
                            cx.module,
                            *range,
                            Code::E0101,
                            "secret() takes the binding's atom".into(),
                        ));
                        None
                    }
                }
            }
            _ => None,
        }
    }

    fn classify(&self, cx: &OpCx, path: &[String], _diags: &mut Vec<Diagnostic>) -> Ref {
        match path {
            [h, field] if h == "host" => b::host_field(field),
            [n] => match cx.bindings.get(n) {
                Some(Bound::Controller) => b::controller(n),
                Some(Bound::Output { step, name, secret }) => b::output(step, name, *secret),
                Some(Bound::Literal(_)) | Some(Bound::Name) => b::param(n),
                None => match cx.scope.probes.get(n) {
                    Some(true) => b::controller(n),
                    Some(false) => b::fact(n),
                    None if cx.scope.loop_vars.contains(n) => b::controller(n),
                    None => b::param(n),
                },
            },
            [alias, name] if cx.scope.aliases.contains_key(alias) => {
                let secret = cx.scope.aliases[alias]
                    .iter()
                    .any(|o| &o.name == name && o.secret);
                b::output(alias, name, secret)
            }
            other => b::param(&other.join(".")),
        }
    }

    /// A fact as a body names it.
    fn fact_ref(&self, cx: &OpCx, e: &Expr, diags: &mut Vec<Diagnostic>) -> Option<FactRef> {
        let (shape, anchor) = self.shape(cx, &Arg::Expr(e.clone()), false, diags)?;
        Some(FactRef { shape, anchor })
    }

    /// The one shape rule (docs/LANGUAGE.md): `file("/p")` is `file:/p`;
    /// `a.b` is `a:b`; `a.b(x)` is `a:b:<x>`, a literal verbatim and a
    /// runtime value as `{name}`; a probe name under `derived:` is
    /// `probe:<name>`; the anchor is the entry's, not the shape's.
    fn shape(
        &self,
        cx: &OpCx,
        a: &Arg,
        derived: bool,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<(String, Option<String>)> {
        match a {
            Arg::Expr(Expr::Call { path, args, range }) => {
                let inst = args.iter().find_map(|x| match x {
                    Arg::Expr(e) => Some(e),
                    _ => None,
                });
                let inst_text = match inst {
                    Some(Expr::Lit {
                        lit: Lit::Str(s), ..
                    }) => self
                        .template(cx, s, diags)
                        .iter()
                        .map(|p| match p {
                            Part::Lit(t) => t.clone(),
                            Part::Ref(r) => format!("{{{}}}", r.label()),
                        })
                        .collect::<String>(),
                    Some(Expr::Ref { path: p, .. }) => format!("{{{}}}", p.join(".")),
                    Some(e) => self.src(cx.module, e.range()),
                    None => {
                        diags.push(self.err(
                            cx.module,
                            *range,
                            Code::E0101,
                            "a fact shape names its instance".into(),
                        ));
                        return None;
                    }
                };
                let anchor = site::kw(args, "anchor").and_then(|k| str_of(&k.value));
                let shape = match path.as_slice() {
                    [one] => format!("{one}:{inst_text}"),
                    many => format!("{}:{inst_text}", many.join(":")),
                };
                Some((shape, anchor))
            }
            Arg::Expr(Expr::Ref { path, .. }) => {
                if derived && path.len() == 1 {
                    Some((format!("probe:{}", path[0]), None))
                } else {
                    Some((path.join(":"), None))
                }
            }
            other => {
                diags.push(self.err(
                    cx.module,
                    other.range(),
                    Code::E0101,
                    "a fact shape is file(...), a dotted name, or a dotted call".into(),
                ));
                None
            }
        }
    }
}

/// Where an op is called from: the module, the (possibly qualified) name,
/// and the call's range for diagnostics.
struct CallSite<'a> {
    module: usize,
    path: &'a [String],
    range: TextRange,
}

struct OpCx<'a> {
    module: usize,
    def: &'a Def,
    bindings: &'a Bindings,
    scope: &'a Scope,
}

const PLAN_OPTS: &[&str] = &[
    "gate",
    "wane",
    "backstop",
    "mode",
    "strictness",
    "exclusivity",
    "fires_by_construction",
    "require",
];
const PRIMS: &[&str] = &[
    "run",
    "write",
    "remove",
    "append",
    "region_set",
    "region_clear",
    "stage",
    "hook",
    "install",
    "release",
];

/// E0208's rule (6.4): restore and compensate are idempotent by
/// construction; a computed body is iff each primitive is, and a run or
/// hook is only when the call says `idempotent: true`.
fn idempotent(cx: &OpCx, body: &[Stmt], undo: &Undo) -> bool {
    match undo {
        Undo::NoUndo | Undo::Restore | Undo::Compensate { .. } => true,
        Undo::Computed { .. } => {
            let Some(line) = super::lines(body).find(|l| l.keyword == "undo") else {
                return true;
            };
            let mut calls: Vec<&Expr> = Vec::new();
            for a in &line.args {
                match a {
                    Arg::Expr(Expr::List { items, .. }) => {
                        calls.extend(items.iter().filter_map(|x| match x {
                            Arg::Expr(e) => Some(e),
                            _ => None,
                        }))
                    }
                    Arg::Expr(e) => calls.push(e),
                    Arg::Kw(_) => {}
                }
            }
            calls.iter().all(|e| match e {
                Expr::Call { path, args, .. }
                    if path.len() == 1 && (path[0] == "run" || path[0] == "hook") =>
                {
                    matches!(
                        site::kw(args, "idempotent").map(|k| &*k.value),
                        Some(Arg::Expr(Expr::Lit {
                            lit: Lit::Bool(true),
                            ..
                        }))
                    )
                }
                _ => true,
            }) && !cx.def.body.is_empty()
        }
    }
}

fn static_locus_host(d: &Def) -> Option<String> {
    super::lines(&d.body)
        .find(|l| l.keyword == "locus")
        .and_then(|l| match l.args.first() {
            Some(Arg::Expr(Expr::Call { path, args, .. })) if path == &["host".to_string()] => {
                match args.first() {
                    Some(Arg::Expr(Expr::Lit {
                        lit: Lit::Str(s), ..
                    })) => Some(crate::ast::unquote(s)),
                    _ => None,
                }
            }
            _ => None,
        })
}

fn pat_text(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit {
            lit: Lit::Atom(a), ..
        } => Some(a.clone()),
        Expr::Lit {
            lit: Lit::Str(s), ..
        } => Some(crate::ast::unquote(s)),
        Expr::Lit {
            lit: Lit::Int(n), ..
        } => Some(n.to_string()),
        Expr::Ref { path, .. } if path.len() == 1 && path[0] == "_" => None,
        Expr::Ref { path, .. } => Some(path.join(".")),
        _ => None,
    }
}

fn first_atom(args: &[Arg]) -> Option<String> {
    args.iter().find_map(atom_of)
}

fn first_bool(args: &[Arg]) -> Option<bool> {
    args.iter().find_map(|a| match a {
        Arg::Expr(Expr::Lit {
            lit: Lit::Bool(b), ..
        }) => Some(*b),
        _ => None,
    })
}

fn first_duration(args: &[Arg]) -> Option<Duration> {
    args.iter().find_map(duration_of)
}

fn atom_of(a: &Arg) -> Option<String> {
    match a {
        Arg::Expr(Expr::Lit {
            lit: Lit::Atom(x), ..
        }) => Some(x.clone()),
        _ => None,
    }
}

fn str_of(a: &Arg) -> Option<String> {
    match a {
        Arg::Expr(Expr::Lit {
            lit: Lit::Str(s), ..
        }) => Some(crate::ast::unquote(s)),
        _ => None,
    }
}

fn int_of(a: &Arg) -> Option<u64> {
    match a {
        Arg::Expr(Expr::Lit {
            lit: Lit::Int(n), ..
        }) => Some(*n),
        _ => None,
    }
}

fn duration_of(a: &Arg) -> Option<Duration> {
    match a {
        Arg::Expr(Expr::Lit {
            lit: Lit::Duration(s),
            ..
        }) => Some(Duration::new(*s)),
        _ => None,
    }
}

fn kw_duration(args: &[Arg], name: &str) -> Option<Duration> {
    site::kw(args, name).and_then(|k| duration_of(&k.value))
}

fn kw_bool(args: &[Arg], name: &str) -> Option<bool> {
    site::kw(args, name).and_then(|k| match &*k.value {
        Arg::Expr(Expr::Lit {
            lit: Lit::Bool(b), ..
        }) => Some(*b),
        _ => None,
    })
}

fn on_lapse(args: &[Arg]) -> OnLapse {
    match site::kw(args, "on_lapse")
        .and_then(|k| atom_of(&k.value))
        .as_deref()
    {
        Some("hold") => OnLapse::Hold,
        _ => OnLapse::Revert,
    }
}
