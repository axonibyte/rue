//! The site (docs/ROADMAP.md 6.3 `site`, Appendix C): the site block and
//! the inventory it names, derived into the checker's `Site` by the rules
//! docs/LANGUAGE.md states.

use std::path::Path;

use rue_core::artifact;
use rue_core::model::{ArtifactLanguage, Authenticator, Duration, HostRecord, Site};
use serde::Deserialize;

use crate::ast::{Arg, Block, Expr, Kw, Lit, Stmt};

/// What an inventory file yields: the checker's host records, the contract
/// facts clause dispatch reads, the authenticators, and the hosts with a
/// scheduler.
#[derive(Debug, Clone, PartialEq)]
pub struct Inventory {
    pub hosts: Vec<HostRecord>,
    pub contracts: Vec<Contract>,
    pub authenticators: Vec<Authenticator>,
    pub scheduled: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct InventoryFile {
    #[serde(default)]
    host: Vec<InvHost>,
    /// A table, so the inventory's own order is the site's (the terms and
    /// the verdict's gate report list authenticators in that order).
    #[serde(default)]
    authenticators: toml::Table,
}

#[derive(Debug, Deserialize)]
struct InvHost {
    name: String,
    #[serde(default)]
    address: String,
    os: String,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    reach: Vec<String>,
    #[serde(default)]
    filesystem: bool,
    #[serde(default)]
    scheduler: Option<String>,
    #[serde(default)]
    artifact: Option<String>,
    #[serde(default)]
    stdin_preamble: Option<bool>,
}

/// A host's contract facts for clause dispatch: the inventory record's
/// fields by name.
#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub name: String,
    pub address: String,
    pub os: String,
    pub roles: Vec<String>,
    pub reach: Vec<String>,
}

/// What the site block declares, before derivation.
#[derive(Debug, Default, Clone)]
pub struct SiteDecl {
    pub inventory: Option<Binding>,
    pub journal: Option<Binding>,
    pub approval: Option<Binding>,
    pub secrets_from: Option<Binding>,
    pub deliver_to: Vec<Binding>,
    pub execute: Vec<Binding>,
    pub scheduler: Option<Binding>,
    pub notify: Option<Binding>,
    pub max_wait: Option<Duration>,
    pub skew_tolerance: Option<Duration>,
    pub identities: Vec<Identity>,
    pub registrars: Vec<Registrar>,
}

/// A binding call: `file("x")`, `hook(:name, transport: :api)`, `local()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub kind: String,
    pub arg: Option<String>,
    pub kws: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub admin: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Registrar {
    pub name: String,
    pub may_register: Vec<String>,
}

fn atom_or_str(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit {
            lit: Lit::Atom(a), ..
        } => Some(a.clone()),
        Expr::Lit {
            lit: Lit::Str(s), ..
        } => Some(crate::ast::unquote(s)),
        Expr::Ref { path, .. } => Some(path.join(".")),
        _ => None,
    }
}

fn binding_of(e: &Expr) -> Option<Binding> {
    match e {
        Expr::Call { path, args, .. } => {
            let mut arg = None;
            let mut kws = Vec::new();
            for a in args {
                match a {
                    Arg::Expr(x) => arg = atom_or_str(x),
                    Arg::Kw(k) => {
                        if let Arg::Expr(x) = &*k.value {
                            if let Some(v) = atom_or_str(x) {
                                kws.push((k.name.clone(), v));
                            }
                        }
                    }
                }
            }
            Some(Binding {
                kind: path.join("."),
                arg,
                kws,
            })
        }
        _ => None,
    }
}

fn bindings_of(a: &Arg) -> Vec<Binding> {
    match a {
        Arg::Expr(Expr::List { items, .. }) => items.iter().flat_map(bindings_of).collect(),
        Arg::Expr(e) => binding_of(e).into_iter().collect(),
        Arg::Kw(k) => bindings_of(&k.value),
    }
}

/// Read the site block.
pub fn declare(block: &Block) -> SiteDecl {
    let mut d = SiteDecl::default();
    for st in &block.body {
        match st {
            Stmt::Line(l) => {
                let first_kw = l.args.first().and_then(|a| match a {
                    Arg::Kw(k) => Some(k),
                    _ => None,
                });
                let bs: Vec<Binding> = l.args.iter().flat_map(bindings_of).collect();
                match (l.keyword.as_str(), first_kw.map(|k| k.name.as_str())) {
                    ("inventory", Some("from")) => d.inventory = bs.into_iter().next(),
                    ("journal", Some("to")) => d.journal = bs.into_iter().next(),
                    ("approval", Some("via")) => d.approval = bs.into_iter().next(),
                    ("secrets", Some("from")) => d.secrets_from = bs.into_iter().next(),
                    ("secrets", Some("deliver_to")) => d.deliver_to = bs,
                    ("execute", Some("via")) => d.execute = bs,
                    ("backstop", Some("scheduler")) => d.scheduler = bs.into_iter().next(),
                    ("notify", Some("via")) => d.notify = bs.into_iter().next(),
                    ("max_wait", _) => d.max_wait = duration_arg(&l.args),
                    ("skew_tolerance", _) => d.skew_tolerance = duration_arg(&l.args),
                    _ => {}
                }
            }
            Stmt::Block(b) if b.keyword == "operators" => {
                for st in &b.body {
                    if let Stmt::Line(l) = st {
                        if l.keyword == "identity" {
                            let name = l
                                .args
                                .first()
                                .and_then(|a| match a {
                                    Arg::Expr(e) => atom_or_str(e),
                                    _ => None,
                                })
                                .unwrap_or_default();
                            let admin = kw_bool(&l.args, "admin").unwrap_or(false);
                            d.identities.push(Identity { name, admin });
                        }
                    }
                }
            }
            Stmt::Block(b) if b.keyword == "hooks" => {
                for st in &b.body {
                    if let Stmt::Line(l) = st {
                        if l.keyword == "registrar" {
                            let name = l
                                .args
                                .first()
                                .and_then(|a| match a {
                                    Arg::Expr(e) => atom_or_str(e),
                                    _ => None,
                                })
                                .unwrap_or_default();
                            let may_register = kw_list(&l.args, "may_register");
                            d.registrars.push(Registrar { name, may_register });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    d
}

fn duration_arg(args: &[Arg]) -> Option<Duration> {
    args.iter().find_map(|a| match a {
        Arg::Expr(Expr::Lit {
            lit: Lit::Duration(s),
            ..
        }) => Some(Duration::new(*s)),
        _ => None,
    })
}

pub fn kw<'a>(args: &'a [Arg], name: &str) -> Option<&'a Kw> {
    args.iter().find_map(|a| match a {
        Arg::Kw(k) if k.name == name => Some(k),
        _ => None,
    })
}

fn kw_bool(args: &[Arg], name: &str) -> Option<bool> {
    match kw(args, name).map(|k| &*k.value) {
        Some(Arg::Expr(Expr::Lit {
            lit: Lit::Bool(b), ..
        })) => Some(*b),
        _ => None,
    }
}

fn kw_list(args: &[Arg], name: &str) -> Vec<String> {
    match kw(args, name).map(|k| &*k.value) {
        Some(Arg::Expr(Expr::List { items, .. })) => items
            .iter()
            .filter_map(|a| match a {
                Arg::Expr(e) => atom_or_str(e),
                _ => None,
            })
            .collect(),
        Some(Arg::Expr(e)) => atom_or_str(e).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The inventory file, read relative to the declaring file's directory.
pub fn read_inventory(dir: &Path, b: &Binding) -> Result<Inventory, String> {
    if b.kind != "file" {
        // A hook inventory (T4) has no file: the hosts come from the hook at
        // run time. Phase 2 reads a sibling inventory.toml as the record the
        // hook would return, which every hook-inventoried tenant keeps.
        let p = dir.join("inventory.toml");
        return read_toml(&p);
    }
    let rel = b.arg.clone().ok_or("inventory from: file() needs a path")?;
    read_toml(&dir.join(rel))
}

fn read_toml(p: &Path) -> Result<Inventory, String> {
    let text = std::fs::read_to_string(p)
        .map_err(|e| format!("cannot read inventory {}: {e}", p.display()))?;
    let inv: InventoryFile =
        toml::from_str(&text).map_err(|e| format!("inventory {}: {e}", p.display()))?;
    let mut hosts = Vec::new();
    let mut contracts = Vec::new();
    let mut scheduled = Vec::new();
    for h in inv.host {
        let artifact = match h.artifact.as_deref() {
            None => None,
            Some("sh") => Some(ArtifactLanguage::Sh),
            Some("powershell") => Some(ArtifactLanguage::Powershell),
            Some("python") => Some(ArtifactLanguage::Python),
            Some(other) => {
                return Err(format!(
                    "inventory {}: host {}: unknown artifact language {other}",
                    p.display(),
                    h.name
                ))
            }
        };
        if h.scheduler.is_some() {
            scheduled.push(h.name.clone());
        }
        hosts.push(HostRecord {
            name: h.name.clone(),
            os: h.os.clone(),
            reach: h.reach.clone(),
            filesystem: h.filesystem,
            stdin_preamble: h.stdin_preamble.unwrap_or(h.filesystem),
            artifact,
        });
        contracts.push(Contract {
            name: h.name,
            address: h.address,
            os: h.os,
            roles: h.roles,
            reach: h.reach,
        });
    }
    let mut auths = Vec::new();
    for (id, a) in inv.authenticators {
        let human = a.get("human").and_then(|v| v.as_bool()).ok_or_else(|| {
            format!(
                "inventory {}: authenticator {id} needs `human = true|false`",
                p.display()
            )
        })?;
        auths.push(Authenticator { id, human });
    }
    Ok(Inventory {
        hosts,
        contracts,
        authenticators: auths,
        scheduled,
    })
}

/// The derived site (docs/LANGUAGE.md, "The site block").
pub fn derive(
    decl: &SiteDecl,
    hosts: Vec<HostRecord>,
    authenticators: Vec<Authenticator>,
    scheduled: Vec<String>,
) -> Site {
    let transports = if decl.execute.is_empty() {
        vec!["ssh".to_string()]
    } else {
        decl.execute
            .iter()
            .filter_map(|b| match b.kind.as_str() {
                "ssh" => Some("ssh".to_string()),
                "local" => Some("local".to_string()),
                "hook" => b
                    .kws
                    .iter()
                    .find(|(k, _)| k == "transport")
                    .map(|(_, v)| v.clone()),
                _ => None,
            })
            .collect()
    };
    let secrets_deliver_to = decl
        .deliver_to
        .iter()
        .map(|b| match b.kind.as_str() {
            "hook" => format!("hook:{}", b.arg.clone().unwrap_or_default()),
            other => other.to_string(),
        })
        .collect();
    let _ = artifact::default_language;
    Site {
        hosts,
        transports,
        authenticators,
        max_wait: decl.max_wait,
        scheduler_present: scheduled,
        secrets_deliver_to,
    }
}
