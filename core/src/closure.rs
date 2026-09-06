//! Closure analysis for a `:target` undo, docs/ROADMAP.md section 5.3
//! (E0202): the undo body references only target-local commands and facts
//! observable on the target, so a backstop artifact can run it with the
//! engine dead. Every primitive declares which of its positions must be
//! target-local (section 6.4); a `hook`, `install` or `release` is a
//! controller act and is never closed.
//!
//! Two positions section 5 does not state, recorded in the not-proven table:
//! plan parameters and host-record fields are bakeable into the artifact and
//! therefore closed; an earlier step's output is never closed, since nothing
//! in section 7.7 persists an output to the target. A Secret reference is
//! not reported here: E0210 is the specific diagnosis and wins.

use crate::body::{Body, FactRef, Prim, Ref, Value};
use crate::model::{Kind, Op, Undo};

/// Why a `:target` undo is not closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unclosed {
    /// A controller primitive (hook, install, release) or a call with a
    /// controller-class argument, at this primitive index.
    ControllerPrim { index: usize, prim: &'static str },
    /// A reference that is not target-local, in a target-local position.
    ControllerRef {
        index: usize,
        position: &'static str,
        r: Ref,
    },
    /// A fact whose shape is bound at runtime.
    RuntimeShape { index: usize, shape: String },
    /// `Restore` over a `Held` footprint: release is controller work.
    HeldRestore { shape: String },
}

/// A one-line account of the finding, for the diagnostic.
pub fn describe(u: &Unclosed) -> String {
    match u {
        Unclosed::ControllerPrim { index, prim } => {
            format!("{prim} is a controller primitive (prim {})", index + 1)
        }
        Unclosed::ControllerRef { index, position, r } => {
            let what = match r {
                Ref::Output { .. } => "an earlier step's output",
                Ref::Controller(_) => "a controller value",
                Ref::Secret(_) => "a secret",
                Ref::Fact(_) | Ref::Param(_) | Ref::Host(_) => "target-local",
            };
            format!("{} is {what} ({position}, prim {})", r.label(), index + 1)
        }
        Unclosed::RuntimeShape { index, shape } => {
            format!("{shape} is bound at runtime (prim {})", index + 1)
        }
        Unclosed::HeldRestore { shape } => {
            format!("{shape} is held; its release is controller work")
        }
    }
}

fn runtime_bound(shape: &str) -> bool {
    shape.contains('{')
}

fn check_refs(index: usize, position: &'static str, v: &Value, out: &mut Vec<Unclosed>) {
    for r in v.refs() {
        if !r.target_local() && !r.is_secret() {
            out.push(Unclosed::ControllerRef {
                index,
                position,
                r: r.clone(),
            });
        }
    }
}

fn check_fact(index: usize, f: &FactRef, out: &mut Vec<Unclosed>) {
    if runtime_bound(&f.shape) {
        out.push(Unclosed::RuntimeShape {
            index,
            shape: f.shape.clone(),
        });
    }
}

/// Everything in a body that a `:target` artifact could not carry out.
pub fn unclosed_body(body: &Body) -> Vec<Unclosed> {
    let mut out = Vec::new();
    for (index, p) in body.iter().enumerate() {
        match p {
            Prim::Run(r) => {
                check_refs(index, "cmd", &Value::Template(r.cmd.clone()), &mut out);
                for e in &r.env {
                    check_refs(index, "env", &e.value, &mut out);
                }
                if let Some(s) = &r.stdin {
                    check_refs(index, "stdin", s, &mut out);
                }
            }
            Prim::Write(w) => {
                check_fact(index, &w.fact, &mut out);
                check_refs(index, "content", &w.content, &mut out);
            }
            Prim::Remove(r) => check_fact(index, &r.fact, &mut out),
            Prim::Append(a) => {
                check_fact(index, &a.fact, &mut out);
                check_refs(index, "line", &a.line, &mut out);
            }
            Prim::RegionSet(r) => {
                check_fact(index, &r.fact, &mut out);
                check_refs(index, "content", &r.content, &mut out);
            }
            Prim::RegionClear(r) => check_fact(index, &r.fact, &mut out),
            Prim::Stage(s) => check_refs(index, "content", &s.content, &mut out),
            Prim::Hook(_) | Prim::Install(_) | Prim::Release(_) => {
                out.push(Unclosed::ControllerPrim {
                    index,
                    prim: p.name(),
                })
            }
            Prim::Call(c) => {
                if c.args
                    .iter()
                    .any(|a| a.class == crate::body::ArgClass::Controller)
                {
                    out.push(Unclosed::ControllerPrim {
                        index,
                        prim: "call",
                    });
                }
                check_refs(index, "cmd", &Value::Template(c.run.clone()), &mut out);
                for a in &c.args {
                    check_refs(index, "arg", &a.value, &mut out);
                }
            }
        }
    }
    out
}

/// Why an op's undo is not closed, for a `:target` undo locus. A `Restore`
/// undo is closed when every footprint shape is static and no kind needs the
/// controller to undo it.
pub fn unclosed_undo(op: &Op) -> Vec<Unclosed> {
    match &op.undo {
        Undo::Restore => {
            let mut out = Vec::new();
            for (index, e) in op.footprint.iter().enumerate() {
                if runtime_bound(&e.shape) {
                    out.push(Unclosed::RuntimeShape {
                        index,
                        shape: e.shape.clone(),
                    });
                }
                if e.kind == Kind::Held {
                    out.push(Unclosed::HeldRestore {
                        shape: e.shape.clone(),
                    });
                }
            }
            out
        }
        Undo::Computed { body, .. } | Undo::Compensate { body, .. } => unclosed_body(body),
        Undo::NoUndo => Vec::new(),
    }
}
