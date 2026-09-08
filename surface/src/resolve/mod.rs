//! The resolver (docs/ROADMAP.md Phase 2 task 4): a `.rue` file, its
//! imports and its inventory to one `PlanIr` per host, the same input the
//! checker reads from a `plan.json`. Reads files (the imports, the
//! inventory); nothing else in the crate does.

pub mod expand;
pub mod site;
pub mod value;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rowan::TextRange;
use rue_core::diagnostics::{Code, Diagnostic, Span};
use rue_core::ir::{PlanIr, IR_VERSION};

use crate::ast::{self, Def, Stmt, Top};

/// What the caller selects.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The inventory host; the plan's owner. Required when the file's
    /// plans are not all for one named host.
    pub host: Option<String>,
    /// The plan name, when the file defines more than one.
    pub plan: Option<String>,
    /// The requester (`--as`); the first declared identity when absent.
    pub requester: Option<String>,
}

/// One loaded file.
#[derive(Debug)]
pub struct Module {
    pub path: PathBuf,
    pub src: String,
    pub file: ast::File,
    /// Import alias to module index.
    pub imports: BTreeMap<String, usize>,
    /// Modules imported without an alias (their names visible unqualified).
    pub open: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct Program {
    pub modules: Vec<Module>,
}

/// A place in a file, for a diagnostic.
pub fn span_of(m: &Module, range: TextRange) -> Span {
    let offset: usize = range.start().into();
    let before = &m.src[..offset.min(m.src.len())];
    Span {
        file: m.path.display().to_string(),
        line: before.matches('\n').count() as u32 + 1,
        col: before
            .rsplit('\n')
            .next()
            .map(|l| l.chars().count())
            .unwrap_or(0) as u32
            + 1,
    }
}

pub(crate) fn diag(code: Code, span: Option<Span>, message: String) -> Diagnostic {
    Diagnostic {
        code,
        span,
        expected: None,
        found: None,
        nearest: None,
        message,
    }
}

impl Program {
    /// Load a file and, transitively, its imports. Parse errors are
    /// returned as they are; an import cycle is E0104; an unreadable
    /// import is E0104 too (the file it names is not there to include).
    pub fn load(root: &Path) -> Result<Program, Vec<Diagnostic>> {
        let mut p = Program::default();
        let mut diags = Vec::new();
        let mut stack = Vec::new();
        p.load_one(root, &mut stack, &mut diags, None);
        if diags.is_empty() {
            Ok(p)
        } else {
            Err(diags)
        }
    }

    fn load_one(
        &mut self,
        path: &Path,
        stack: &mut Vec<PathBuf>,
        diags: &mut Vec<Diagnostic>,
        from: Option<Span>,
    ) -> Option<usize> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        // A file still being loaded is a cycle; one loaded before is shared.
        if stack.contains(&canonical) {
            let cycle: Vec<String> = stack
                .iter()
                .skip_while(|p| **p != canonical)
                .map(|p| p.display().to_string())
                .collect();
            diags.push(diag(
                Code::E0104,
                from,
                format!(
                    "import cycle: {} imports {}",
                    cycle.join(" imports "),
                    canonical.display()
                ),
            ));
            return None;
        }
        if let Some(i) = self.modules.iter().position(|m| m.path == canonical) {
            return Some(i);
        }
        let src = match std::fs::read_to_string(&canonical) {
            Ok(s) => s,
            Err(e) => {
                diags.push(diag(
                    Code::E0104,
                    from,
                    format!("cannot read {}: {e}", canonical.display()),
                ));
                return None;
            }
        };
        let parsed = crate::parse(&src, &canonical.display().to_string());
        diags.extend(parsed.diagnostics.iter().cloned());
        let file = ast::lower(&parsed.root);
        let index = self.modules.len();
        self.modules.push(Module {
            path: canonical.clone(),
            src,
            file,
            imports: BTreeMap::new(),
            open: Vec::new(),
        });
        stack.push(canonical.clone());
        let dir = canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let imports: Vec<ast::Import> = self.modules[index]
            .file
            .tops
            .iter()
            .filter_map(|t| match t {
                Top::Import(i) => Some(i.clone()),
                _ => None,
            })
            .collect();
        for imp in imports {
            let span = Some(span_of(&self.modules[index], imp.range));
            if let Some(child) = self.load_one(&dir.join(&imp.path), stack, diags, span) {
                match &imp.alias {
                    Some(a) => {
                        self.modules[index].imports.insert(a.clone(), child);
                    }
                    None => self.modules[index].open.push(child),
                }
            }
        }
        stack.pop();
        Some(index)
    }

    pub fn root(&self) -> &Module {
        &self.modules[0]
    }

    /// Every definition of `keyword` (defop, defplan, defprobe, ...) in a
    /// module, in file order.
    pub fn defs<'a>(&'a self, module: usize, keyword: &str) -> Vec<(usize, &'a Def)> {
        self.modules[module]
            .file
            .tops
            .iter()
            .filter_map(|t| match t {
                Top::Def(d) if d.keyword == keyword => Some((module, d)),
                _ => None,
            })
            .collect()
    }

    /// Resolve a possibly qualified name to the clauses defined for it:
    /// `t3.pf_allow` in the module `t3` names; a bare name in this module,
    /// else in an unaliased import.
    pub fn lookup<'a>(
        &'a self,
        module: usize,
        keyword: &str,
        path: &[String],
    ) -> Vec<(usize, &'a Def)> {
        match path {
            [alias, name] => match self.modules[module].imports.get(alias) {
                Some(&m) => self
                    .defs(m, keyword)
                    .into_iter()
                    .filter(|(_, d)| d.name == *name)
                    .collect(),
                None => Vec::new(),
            },
            [name] => {
                let local: Vec<_> = self
                    .defs(module, keyword)
                    .into_iter()
                    .filter(|(_, d)| d.name == *name)
                    .collect();
                if !local.is_empty() {
                    return local;
                }
                for &m in &self.modules[module].open {
                    let found: Vec<_> = self
                        .defs(m, keyword)
                        .into_iter()
                        .filter(|(_, d)| d.name == *name)
                        .collect();
                    if !found.is_empty() {
                        return found;
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Every name of `keyword` visible from a module, for E0102's suggestion.
    pub fn visible_names(&self, module: usize, keyword: &str) -> Vec<String> {
        let mut v: Vec<String> = self
            .defs(module, keyword)
            .iter()
            .map(|(_, d)| d.name.clone())
            .collect();
        for (alias, &m) in &self.modules[module].imports {
            v.extend(
                self.defs(m, keyword)
                    .iter()
                    .map(|(_, d)| format!("{alias}.{}", d.name)),
            );
        }
        for &m in &self.modules[module].open {
            v.extend(self.defs(m, keyword).iter().map(|(_, d)| d.name.clone()));
        }
        v.sort();
        v.dedup();
        v
    }

    /// The site block that governs a module: its own, else the one of
    /// exactly one imported module (E0103 when two imports bring one and
    /// none is local).
    pub fn site_of(&self, module: usize) -> Result<(usize, &ast::Block), Box<Diagnostic>> {
        self.site_of_visited(module, &mut Vec::new())
    }

    fn site_of_visited(
        &self,
        module: usize,
        visited: &mut Vec<usize>,
    ) -> Result<(usize, &ast::Block), Box<Diagnostic>> {
        if visited.contains(&module) {
            return Err(Box::new(diag(
                Code::E0104,
                None,
                "import cycle".to_string(),
            )));
        }
        visited.push(module);
        let own = self.modules[module].file.tops.iter().find_map(|t| match t {
            Top::Site(b) => Some(b),
            _ => None,
        });
        if let Some(b) = own {
            return Ok((module, b));
        }
        let mut found = Vec::new();
        let mut children: Vec<usize> = self.modules[module].imports.values().copied().collect();
        children.extend(self.modules[module].open.iter().copied());
        for m in children {
            if let Ok(s) = self.site_of_visited(m, visited) {
                found.push(s);
            }
        }
        match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err(Box::new(diag(
                Code::E0604,
                None,
                format!(
                    "{}: no site block, here or in an import",
                    self.modules[module].path.display()
                ),
            ))),
            _ => Err(Box::new(diag(
                Code::E0103,
                None,
                format!(
                    "{}: two imported files declare a site and this one declares none",
                    self.modules[module].path.display()
                ),
            ))),
        }
    }
}

/// Resolve a file to the plan IR for one host.
pub fn resolve(path: &Path, opts: &Options) -> Result<PlanIr, Vec<Diagnostic>> {
    let program = Program::load(path)?;
    let mut diags = Vec::new();
    // The site.
    let (site_module, site_block) = program.site_of(0).map_err(|d| vec![*d])?;
    let decl = site::declare(site_block);
    diags.extend(site::validate(&decl, site_block, &|range| {
        diag(
            Code::E0601,
            Some(span_of(&program.modules[site_module], range)),
            String::new(),
        )
    }));
    // A site that does not stand is not read further: its inventory line
    // may be the very binding refused.
    if !diags.is_empty() {
        return Err(diags);
    }
    let site_dir = program.modules[site_module]
        .path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let inventory = match &decl.inventory {
        Some(b) => site::read_inventory(&site_dir, b),
        None => Err("no inventory declared".to_string()),
    };
    let inventory = match inventory {
        Ok(x) => x,
        Err(e) => {
            return Err(vec![diag(
                Code::E0601,
                Some(span_of(&program.modules[site_module], site_block.range)),
                e,
            )])
        }
    };
    let contracts = inventory.contracts;
    let site = site::derive(
        &decl,
        inventory.hosts,
        inventory.authenticators,
        inventory.scheduled,
    );
    let requester = opts
        .requester
        .clone()
        .or_else(|| decl.identities.first().map(|i| i.name.clone()))
        .unwrap_or_else(|| "requester".to_string());

    // The plan.
    let plans = program.defs(0, "defplan");
    let names: Vec<String> = {
        let mut v: Vec<String> = plans.iter().map(|(_, d)| d.name.clone()).collect();
        v.dedup();
        v
    };
    let plan_name = match (&opts.plan, names.as_slice()) {
        (Some(p), _) => p.clone(),
        (None, [one]) => one.clone(),
        (None, []) => {
            return Err(vec![diag(
                Code::E0102,
                None,
                format!("{}: no plan is defined", program.root().path.display()),
            )])
        }
        (None, many) => {
            return Err(vec![diag(
                Code::E0102,
                None,
                format!(
                    "the file defines several plans ({}); name one with --plan",
                    many.join(", ")
                ),
            )])
        }
    };
    let clauses: Vec<(usize, &Def)> = plans
        .into_iter()
        .filter(|(_, d)| d.name == plan_name)
        .collect();
    if clauses.is_empty() {
        let mut d = diag(Code::E0102, None, format!("no plan named {plan_name}"));
        d.nearest = rue_core::diagnostics::nearest(&plan_name, names.iter().map(String::as_str));
        return Err(vec![d]);
    }
    let host_name = match &opts.host {
        Some(h) => h.clone(),
        None => {
            // A plan whose every clause names one host needs no --host.
            let named: Vec<String> = clauses
                .iter()
                .filter_map(|(_, d)| expand::pattern_host_name(d))
                .collect();
            match named.as_slice() {
                [h] if clauses.len() == 1 => h.clone(),
                _ => {
                    return Err(vec![diag(
                        Code::E0112,
                        None,
                        "the plan's host is not fixed by its pattern; name one with --host"
                            .to_string(),
                    )])
                }
            }
        }
    };
    let contract = match contracts.iter().find(|c| c.name == host_name) {
        Some(c) => c.clone(),
        None => {
            let mut d = diag(
                Code::E0102,
                None,
                format!("host {host_name} is not in the inventory"),
            );
            d.nearest = rue_core::diagnostics::nearest(
                &host_name,
                contracts.iter().map(|c| c.name.as_str()),
            );
            return Err(vec![d]);
        }
    };
    let cx = expand::Context {
        program: &program,
        contracts: &contracts,
        site: &site,
    };
    let plan = cx.expand_plan(&clauses, &contract, &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }
    Ok(PlanIr {
        ir_version: IR_VERSION,
        requester,
        site,
        plan: plan.expect("a plan when no diagnostic was raised"),
    })
}

/// Statements of a body as lines, for the site block and definitions.
pub(crate) fn lines(body: &[Stmt]) -> impl Iterator<Item = &ast::Line> {
    body.iter().filter_map(|s| match s {
        Stmt::Line(l) => Some(l),
        _ => None,
    })
}
