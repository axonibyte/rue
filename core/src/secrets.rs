//! Secret placement, docs/ROADMAP.md sections 5.3 and 5.8. A secret is a
//! reference whose origin is a `secrets from:` binding or an earlier step's
//! secret output; the rules here read where such a reference stands, never
//! what it holds. Core decides what the structure decides: the string
//! position of a `run` (E0209), the undo body of a `:target` undo (E0210),
//! the `env:`/`stdin:` channel on a host whose executor cannot carry it
//! (E0211, static hosts only), and a `reestablish` that would produce the
//! secret again (E0206, as a structural re-run of a `do` primitive). Routing
//! to a sink (E0411) needs sink declarations the site does not model.

use crate::body::{Body, Prim, Ref, Value};

/// Where a secret reference was found: the primitive's index and the
/// argument position, as the surface names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub index: usize,
    pub position: &'static str,
    pub r: Ref,
}

/// Every value of a primitive with the name of its position.
fn positions(p: &Prim) -> Vec<(&'static str, Value)> {
    match p {
        Prim::Run(r) => {
            let mut v = vec![("cmd", Value::Template(r.cmd.clone()))];
            v.extend(r.env.iter().map(|e| ("env", e.value.clone())));
            if let Some(s) = &r.stdin {
                v.push(("stdin", s.clone()));
            }
            v
        }
        Prim::Write(w) => vec![("content", w.content.clone())],
        Prim::Remove(_) | Prim::RegionClear(_) | Prim::Install(_) | Prim::Release(_) => vec![],
        Prim::Append(a) => vec![("line", a.line.clone())],
        Prim::RegionSet(r) => vec![("content", r.content.clone())],
        Prim::Stage(s) => vec![("content", s.content.clone())],
        Prim::Hook(h) => h.args.iter().map(|a| ("arg", a.value.clone())).collect(),
        Prim::Call(c) => {
            let mut v = vec![("cmd", Value::Template(c.run.clone()))];
            v.extend(c.args.iter().map(|a| ("arg", a.value.clone())));
            v
        }
    }
}

fn first_secret(index: usize, position: &'static str, v: &Value) -> Option<Found> {
    v.refs().into_iter().find(|r| r.is_secret()).map(|r| Found {
        index,
        position,
        r: r.clone(),
    })
}

/// The first secret reference in a body, in any position.
pub fn anywhere(body: &Body) -> Option<Found> {
    body.iter().enumerate().find_map(|(i, p)| {
        positions(p)
            .iter()
            .find_map(|(pos, v)| first_secret(i, pos, v))
    })
}

/// The first secret reference interpolated into a `run` (or `call`) string.
pub fn in_run_string(body: &Body) -> Option<Found> {
    body.iter().enumerate().find_map(|(i, p)| {
        positions(p)
            .iter()
            .filter(|(pos, _)| *pos == "cmd")
            .find_map(|(pos, v)| first_secret(i, pos, v))
    })
}

/// The first secret reference carried by `env:` or `stdin:`, the channels
/// the executor implements as the stdin preamble.
pub fn in_channel(body: &Body) -> Option<Found> {
    body.iter().enumerate().find_map(|(i, p)| {
        positions(p)
            .iter()
            .filter(|(pos, _)| *pos == "env" || *pos == "stdin")
            .find_map(|(pos, v)| first_secret(i, pos, v))
    })
}

/// The index of the first `reestablish` primitive that is structurally one
/// of the op's `do` primitives: re-running it would produce the output again.
pub fn reruns(do_: &Body, reestablish: &Body) -> Option<usize> {
    reestablish.iter().position(|p| do_.contains(p))
}
