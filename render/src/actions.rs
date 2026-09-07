//! A covered step's undo as language-neutral actions, docs/ROADMAP.md
//! sections 5.2 and 7.7: what the artifact does for each footprint entry
//! of a `:restore` undo, or for each primitive of a computed or
//! compensating body, with every value resolved from the bindings and the
//! `run` text already quoted for the host's shell. The three templates
//! spell the same actions in their own language.

use std::collections::BTreeMap;

use rue_core::artifact::Shell;
use rue_core::body::{FactRef, Part, Prim, Ref, Template, Value};
pub use rue_core::model::Kind;
use rue_core::model::{Drift, FootprintEntry, HostRecord, Op, Undo};

use crate::quote::{self, Family, Unquotable};
use crate::RenderError;

/// The values a render bakes in: the plan's parameters as the request
/// bound them, and host-record fields beyond name and os (`address`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings {
    pub params: BTreeMap<String, String>,
    pub host_fields: BTreeMap<String, String>,
}

/// A file fact of a step: the path the shape names and the snapshot index
/// its `Modified` or `Region` entry gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFact {
    pub kind: Kind,
    pub path: String,
    pub anchor: Option<String>,
    /// The entry's index in the footprint; the snapshot lives at
    /// `snapshots/<step>/<k>`.
    pub k: usize,
}

/// One thing the artifact does when it undoes a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// An `Owned` file: remove it.
    Remove {
        path: String,
    },
    /// A `Region`: strip between the markers; when they are damaged,
    /// restore the whole file from the snapshot under `:clobber` unless a
    /// sibling instance holds a region on it, else defer.
    StripRegion {
        path: String,
        anchor: String,
        k: usize,
    },
    /// A `Modified` file: restore it from the snapshot.
    RestoreSnapshot {
        path: String,
        k: usize,
    },
    /// A `run` body: the text, interpolations quoted for the host's shell.
    Run {
        command: String,
    },
    Write {
        path: String,
        content: String,
    },
    Append {
        path: String,
        line: String,
    },
    RegionSet {
        path: String,
        anchor: String,
        content: String,
    },
    RegionClear {
        path: String,
        anchor: String,
    },
}

/// A covered step, ready for a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub n: u32,
    pub id: String,
    pub drift: Drift,
    /// File facts whose post-do hash the drift check compares under
    /// `:defer` (and reports as clobbered under `:clobber`).
    pub files: Vec<FileFact>,
    pub actions: Vec<Action>,
}

/// The path a `file:` shape names, if it is one.
pub fn file_path(shape: &str) -> Option<&str> {
    shape.strip_prefix("file:")
}

/// A footprint entry as a file fact, when it is a static `file:` shape of
/// a kind the artifact observes. A non-file fact (`winfw:rule:...`,
/// `bmc:account:...`) cannot be observed by a script and is undone as if
/// intact; only a `:restore` undo needs it to be a file (below).
fn file_fact(k: usize, e: &FootprintEntry) -> Option<FileFact> {
    if !matches!(e.kind, Kind::Owned | Kind::Region | Kind::Modified) {
        return None;
    }
    match file_path(&e.shape) {
        Some(path) if !path.contains('{') => Some(FileFact {
            kind: e.kind,
            path: path.to_string(),
            anchor: e.anchor.clone(),
            k,
        }),
        _ => None,
    }
}

/// Resolve one reference to the text it bakes to.
fn resolve(step: u32, r: &Ref, host: &HostRecord, b: &Bindings) -> Result<String, RenderError> {
    let unbound = |name: String| RenderError::Unbound { step, name };
    match r {
        Ref::Param(n) => b.params.get(n).cloned().ok_or_else(|| unbound(n.clone())),
        Ref::Host(f) => match f.as_str() {
            "name" => Ok(host.name.clone()),
            "os" => Ok(host.os.clone()),
            _ => b
                .host_fields
                .get(f)
                .cloned()
                .ok_or_else(|| unbound(format!("host.{f}"))),
        },
        // Observable on the target at run time, but an artifact runs no
        // probe: not bakeable this unit (recorded as not proven).
        Ref::Fact(_) | Ref::Output { .. } | Ref::Controller(_) | Ref::Secret(_) => {
            Err(RenderError::NotBakeable {
                step,
                what: format!("reference {}", r.label()),
            })
        }
    }
}

/// A value as text, with no quoting (it is baked as a whole).
fn text_of(step: u32, v: &Value, host: &HostRecord, b: &Bindings) -> Result<String, RenderError> {
    match v {
        Value::Lit(s) => Ok(s.clone()),
        Value::Ref(r) => resolve(step, r, host, b),
        Value::Template(parts) => parts
            .iter()
            .map(|p| match p {
                Part::Lit(s) => Ok(s.clone()),
                Part::Ref(r) => resolve(step, r, host, b),
            })
            .collect(),
    }
}

fn shell_family(shell: Shell) -> Family {
    match shell {
        Shell::Posix => Family::Posix,
        Shell::Powershell => Family::Powershell,
    }
}

/// A `run` template as shell text: literal parts verbatim, every
/// interpolation quoted for the host's shell.
fn run_text(
    step: u32,
    cmd: &Template,
    shell: Shell,
    host: &HostRecord,
    b: &Bindings,
) -> Result<String, RenderError> {
    let mut out = String::new();
    for p in cmd {
        match p {
            Part::Lit(s) => out.push_str(s),
            Part::Ref(r) => {
                let v = resolve(step, r, host, b)?;
                out.push_str(
                    &quote::quote(shell_family(shell), &v).map_err(|e| unquotable(step, e))?,
                );
            }
        }
    }
    Ok(out)
}

fn unquotable(step: u32, e: Unquotable) -> RenderError {
    RenderError::Unquotable { step, inner: e }
}

fn static_path(step: u32, f: &FactRef) -> Result<String, RenderError> {
    match file_path(&f.shape) {
        Some(p) if !p.contains('{') => Ok(p.to_string()),
        _ => Err(RenderError::Unobservable {
            step,
            shape: f.shape.clone(),
        }),
    }
}

fn anchor_of(step: u32, f: &FactRef) -> Result<String, RenderError> {
    f.anchor.clone().ok_or_else(|| RenderError::NotBakeable {
        step,
        what: format!("region primitive on {} without an anchor", f.shape),
    })
}

/// The actions of a covered step.
pub fn step_actions(
    n: u32,
    op: &Op,
    shell: Shell,
    host: &HostRecord,
    b: &Bindings,
) -> Result<Step, RenderError> {
    let drift = op.effective_drift().unwrap_or(Drift::Clobber);
    let files: Vec<FileFact> = op
        .footprint
        .iter()
        .enumerate()
        .filter_map(|(k, e)| file_fact(k, e))
        .collect();
    let actions = match &op.undo {
        Undo::NoUndo => Vec::new(),
        Undo::Restore => {
            // A restore acts on every entry, so every Owned, Region or
            // Modified entry must be a file the artifact can act on.
            for e in &op.footprint {
                if matches!(e.kind, Kind::Owned | Kind::Region | Kind::Modified)
                    && !files
                        .iter()
                        .any(|f| Some(f.path.as_str()) == file_path(&e.shape))
                {
                    return Err(RenderError::Unobservable {
                        step: n,
                        shape: e.shape.clone(),
                    });
                }
            }
            let mut v = Vec::new();
            for f in &files {
                v.push(match f.kind {
                    Kind::Owned => Action::Remove {
                        path: f.path.clone(),
                    },
                    Kind::Region => Action::StripRegion {
                        path: f.path.clone(),
                        anchor: f.anchor.clone().ok_or_else(|| RenderError::NotBakeable {
                            step: n,
                            what: format!("region {} without an anchor", f.path),
                        })?,
                        k: f.k,
                    },
                    Kind::Modified => Action::RestoreSnapshot {
                        path: f.path.clone(),
                        k: f.k,
                    },
                    _ => unreachable!("files holds Owned, Region and Modified entries only"),
                });
            }
            for e in &op.footprint {
                if e.kind == Kind::Held {
                    return Err(RenderError::NotBakeable {
                        step: n,
                        what: format!("held {} (release is controller work)", e.shape),
                    });
                }
            }
            v
        }
        Undo::Computed { body, .. } | Undo::Compensate { body, .. } => {
            let mut v = Vec::new();
            for p in body {
                v.push(match p {
                    Prim::Run(r) => {
                        if !r.env.is_empty() || r.stdin.is_some() {
                            return Err(RenderError::NotBakeable {
                                step: n,
                                what: "run with env: or stdin: (the preamble is engine work)"
                                    .into(),
                            });
                        }
                        Action::Run {
                            command: run_text(n, &r.cmd, shell, host, b)?,
                        }
                    }
                    Prim::Call(c) => {
                        if c.args
                            .iter()
                            .any(|a| a.class == rue_core::body::ArgClass::Controller)
                        {
                            return Err(RenderError::NotBakeable {
                                step: n,
                                what: format!("call {} with a controller-class argument", c.prim),
                            });
                        }
                        Action::Run {
                            command: run_text(n, &c.run, shell, host, b)?,
                        }
                    }
                    Prim::Write(w) => Action::Write {
                        path: static_path(n, &w.fact)?,
                        content: text_of(n, &w.content, host, b)?,
                    },
                    Prim::Remove(r) => Action::Remove {
                        path: static_path(n, &r.fact)?,
                    },
                    Prim::Append(a) => Action::Append {
                        path: static_path(n, &a.fact)?,
                        line: text_of(n, &a.line, host, b)?,
                    },
                    Prim::RegionSet(r) => Action::RegionSet {
                        path: static_path(n, &r.fact)?,
                        anchor: anchor_of(n, &r.fact)?,
                        content: text_of(n, &r.content, host, b)?,
                    },
                    Prim::RegionClear(r) => Action::RegionClear {
                        path: static_path(n, &r.fact)?,
                        anchor: anchor_of(n, &r.fact)?,
                    },
                    Prim::Stage(_) | Prim::Hook(_) | Prim::Install(_) | Prim::Release(_) => {
                        return Err(RenderError::NotBakeable {
                            step: n,
                            what: format!("{} is a controller primitive", p.name()),
                        });
                    }
                });
            }
            v
        }
    };
    Ok(Step {
        n,
        id: op.id.clone(),
        drift,
        files,
        actions,
    })
}
