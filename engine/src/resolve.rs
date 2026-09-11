//! Resolution: a body's references to the values the engine holds, so an
//! executor receives strings and secrecy and never a `Ref`.
//!
//! Where each reference resolves (section 6.4's classification, which the
//! front end already made): a parameter from the request's bindings; a host
//! field from the step's host; an earlier step's output from what that step
//! produced; a controller value from the engine's own bindings (a
//! `:controller` probe's fact, a `repeat over:` variable); a `secrets from:`
//! binding from the secret source; a fact from the last observation of it.
//! A reference nothing binds is `Unbound`, which is a refusal, not a guess.
//!
//! A fact's shape is resolved too. The front end writes a runtime value in
//! a shape as `{name}` (`guest.state(g)` is `guest:state:{g}`), and the
//! engine reads, writes, snapshots and undoes the fact that value names --
//! never a file called `{g}`, and never one fact for every iteration of a
//! repeat.

use std::collections::BTreeMap;
use std::fmt;

use rue_core::body::{Body, Part, Prim, Ref, Template, Value};
use rue_core::model::{Op, Undo};

use crate::executor::{RPrim, Resolved};
use crate::host::Host;

/// Everything a reference may resolve to at one step.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub params: BTreeMap<String, String>,
    pub outputs: BTreeMap<String, String>,
    pub controller: BTreeMap<String, String>,
    pub facts: BTreeMap<String, String>,
    /// Values of `secrets from:` bindings; every one a secret.
    pub secrets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unbound {
    pub name: String,
}

impl fmt::Display for Unbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nothing binds {}", self.name)
    }
}

fn resolve_ref(r: &Ref, host: &Host, env: &Env) -> Result<Resolved, Unbound> {
    let unbound = || Unbound { name: r.label() };
    match r {
        Ref::Param(n) => env.params.get(n).map(|t| Resolved::plain(t)),
        Ref::Host(f) => host.field(f).map(|t| Resolved::plain(&t)),
        Ref::Output { step, name, secret } => {
            env.outputs
                .get(&format!("{step}.{name}"))
                .map(|t| Resolved {
                    text: t.clone(),
                    secret: *secret,
                })
        }
        Ref::Controller(n) => env.controller.get(n).map(|t| Resolved::plain(t)),
        Ref::Fact(n) => env.facts.get(n).map(|t| Resolved::plain(t)),
        Ref::Secret(n) => env.secrets.get(n).map(|t| Resolved {
            text: t.clone(),
            secret: true,
        }),
    }
    .ok_or_else(unbound)
}

/// A value by the label the front end wrote in a shape: a host field as
/// `host.<field>`, an output as `<step>.<name>`, otherwise a controller
/// value (a repeat variable shadows a parameter of its name, as it does in
/// a body), a parameter, or an observed fact.
fn by_label(label: &str, host: &Host, env: &Env) -> Option<String> {
    if let Some(f) = label.strip_prefix("host.") {
        return host.field(f);
    }
    env.controller
        .get(label)
        .or_else(|| env.outputs.get(label))
        .or_else(|| env.params.get(label))
        .or_else(|| env.facts.get(label))
        .cloned()
}

/// A shape with every `{label}` in it replaced by the value it names. A
/// shape with none is returned as it is.
pub fn instantiate(shape: &str, host: &Host, env: &Env) -> Result<String, Unbound> {
    if !shape.contains('{') {
        return Ok(shape.to_string());
    }
    let mut out = String::new();
    let mut rest = shape;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return Ok(out);
        };
        let label = &after[..close];
        out.push_str(&by_label(label, host, env).ok_or_else(|| Unbound {
            name: label.to_string(),
        })?);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// An op with its footprint and undo preconditions instantiated: the facts
/// one application of it touches. Bodies are instantiated as they are
/// resolved, by `resolve_body`.
pub fn concrete_op(op: &Op, host: &Host, env: &Env) -> Result<Op, Unbound> {
    let mut o = op.clone();
    for e in o.footprint.iter_mut() {
        e.shape = instantiate(&e.shape, host, env)?;
    }
    if let Undo::Computed { undo_pre, .. } | Undo::Compensate { undo_pre, .. } = &mut o.undo {
        for s in undo_pre.iter_mut() {
            *s = instantiate(s, host, env)?;
        }
    }
    Ok(o)
}

pub fn resolve_template(t: &Template, host: &Host, env: &Env) -> Result<Resolved, Unbound> {
    let mut text = String::new();
    let mut secret = false;
    for p in t {
        match p {
            Part::Lit(s) => text.push_str(s),
            Part::Ref(r) => {
                let v = resolve_ref(r, host, env)?;
                text.push_str(&v.text);
                secret |= v.secret;
            }
        }
    }
    Ok(Resolved { text, secret })
}

pub fn resolve_value(v: &Value, host: &Host, env: &Env) -> Result<Resolved, Unbound> {
    match v {
        Value::Lit(s) => Ok(Resolved::plain(s)),
        Value::Ref(r) => resolve_ref(r, host, env),
        Value::Template(t) => resolve_template(t, host, env),
    }
}

pub fn resolve_body(body: &Body, host: &Host, env: &Env) -> Result<Vec<RPrim>, Unbound> {
    body.iter().map(|p| resolve_prim(p, host, env)).collect()
}

fn resolve_prim(p: &Prim, host: &Host, env: &Env) -> Result<RPrim, Unbound> {
    Ok(match p {
        Prim::Run(r) => RPrim::Run {
            cmd: resolve_template(&r.cmd, host, env)?,
            env: r
                .env
                .iter()
                .map(|e| Ok((e.name.clone(), resolve_value(&e.value, host, env)?)))
                .collect::<Result<_, Unbound>>()?,
            stdin: match &r.stdin {
                Some(v) => Some(resolve_value(v, host, env)?),
                None => None,
            },
        },
        Prim::Write(w) => RPrim::Write {
            shape: instantiate(&w.fact.shape, host, env)?,
            content: resolve_value(&w.content, host, env)?,
        },
        Prim::Remove(r) => RPrim::Remove {
            shape: instantiate(&r.fact.shape, host, env)?,
        },
        Prim::Append(a) => RPrim::Append {
            shape: instantiate(&a.fact.shape, host, env)?,
            line: resolve_value(&a.line, host, env)?,
        },
        Prim::RegionSet(r) => RPrim::RegionSet {
            shape: instantiate(&r.fact.shape, host, env)?,
            anchor: r.fact.anchor.clone(),
            content: resolve_value(&r.content, host, env)?,
        },
        Prim::RegionClear(r) => RPrim::RegionClear {
            shape: instantiate(&r.fact.shape, host, env)?,
            anchor: r.fact.anchor.clone(),
        },
        Prim::Stage(s) => RPrim::Stage {
            name: s.name.clone(),
            content: resolve_value(&s.content, host, env)?,
            mode: s.mode,
        },
        Prim::Hook(h) => RPrim::Hook {
            name: h.name.clone(),
            args: h
                .args
                .iter()
                .map(|a| Ok((a.name.clone(), resolve_value(&a.value, host, env)?)))
                .collect::<Result<_, Unbound>>()?,
        },
        Prim::Install(i) => RPrim::Install {
            name: i.name.clone(),
        },
        Prim::Release(r) => RPrim::Release {
            name: r.name.clone(),
        },
        Prim::Call(c) => RPrim::Call {
            prim: c.prim.clone(),
            cmd: resolve_template(&c.run, host, env)?,
        },
    })
}
