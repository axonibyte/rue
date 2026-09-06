//! Bodies and primitives, docs/ROADMAP.md sections 5.3 and 6.4. A body is a
//! list of primitives; the checker sees primitives, never shell. `run`
//! strings are the only place shell text exists, and they are templates of
//! literal parts and references, so what a value is and where it comes from
//! are structure, not text.
//!
//! A [`Ref`] names a value's origin, which is what closure (E0202) and secret
//! placement (E0209, E0210, E0211) read: a fact observable on the step's
//! host, a plan parameter, a host-record field, an earlier step's output
//! (secret or not), a controller-side value (a `:controller` probe's fact, a
//! `repeat over:` variable, an engine-held value), or a `secrets from:`
//! binding. Secrecy has exactly two structural sources and no flag.
//!
//! Every primitive is a named struct that refuses unknown fields, because an
//! enum's struct variants cannot.

use serde::{Deserialize, Serialize};

pub type Body = Vec<Prim>;

/// A footprint fact as a primitive names it: the shape string the footprint
/// uses and, for a region, its anchor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactRef {
    pub shape: String,
    pub anchor: Option<String>,
}

/// Where a value comes from. Core trusts the front end's classification and
/// decides nothing about the world.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ref {
    /// A fact observable on the step's host (a `:target` probe).
    Fact(String),
    /// A plan parameter, bound at request.
    Param(String),
    /// A field of the step's host record (address, name, os).
    Host(String),
    /// An earlier step's output, `alias.name`.
    Output {
        step: String,
        name: String,
        secret: bool,
    },
    /// A value that exists only on the controller: a `:controller` probe's
    /// fact, a `repeat over:` variable, an engine-held value.
    Controller(String),
    /// A `secrets from:` binding; always a Secret.
    Secret(String),
}

impl Ref {
    pub fn is_secret(&self) -> bool {
        matches!(self, Ref::Output { secret: true, .. } | Ref::Secret(_))
    }

    /// Bakeable into a target-side artifact: observable there, or known at
    /// install (a plan parameter, a host-record field).
    pub fn target_local(&self) -> bool {
        matches!(self, Ref::Fact(_) | Ref::Param(_) | Ref::Host(_))
    }

    /// The name as `explain` spells it.
    pub fn label(&self) -> String {
        match self {
            Ref::Fact(n) | Ref::Param(n) | Ref::Controller(n) | Ref::Secret(n) => n.clone(),
            Ref::Host(f) => format!("host.{f}"),
            Ref::Output { step, name, .. } => format!("{step}.{name}"),
        }
    }
}

/// One piece of an interpolated string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Lit(String),
    Ref(Ref),
}

pub type Template = Vec<Part>;

/// A value: a literal, a reference, or an interpolated string. Secret iff any
/// reference in it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Lit(String),
    Ref(Ref),
    Template(Template),
}

impl Value {
    pub fn refs(&self) -> Vec<&Ref> {
        match self {
            Value::Lit(_) => Vec::new(),
            Value::Ref(r) => vec![r],
            Value::Template(parts) => template_refs(parts),
        }
    }

    pub fn is_secret(&self) -> bool {
        self.refs().iter().any(|r| r.is_secret())
    }
}

pub fn template_refs(parts: &[Part]) -> Vec<&Ref> {
    parts
        .iter()
        .filter_map(|p| match p {
            Part::Ref(r) => Some(r),
            Part::Lit(_) => None,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvVar {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KwArg {
    pub name: String,
    pub value: Value,
}

/// Which values a defprim argument may carry: the input to closure analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgClass {
    TargetLocal,
    Controller,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassedArg {
    pub name: String,
    pub class: ArgClass,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub cmd: Template,
    pub env: Vec<EnvVar>,
    pub stdin: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Write {
    pub fact: FactRef,
    pub content: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remove {
    pub fact: FactRef,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Append {
    pub fact: FactRef,
    pub line: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionSet {
    pub fact: FactRef,
    pub content: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionClear {
    pub fact: FactRef,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub name: String,
    pub content: Value,
    pub mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    pub name: String,
    pub args: Vec<KwArg>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub name: String,
}

/// A tenant defprim call: the expanded run template (what the renderer
/// emits) and the arguments with their declared classes (what closure
/// reads).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub prim: String,
    pub run: Template,
    pub args: Vec<ClassedArg>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Prim {
    Run(Run),
    Write(Write),
    Remove(Remove),
    Append(Append),
    RegionSet(RegionSet),
    RegionClear(RegionClear),
    Stage(Stage),
    Hook(Hook),
    Install(Install),
    Release(Release),
    Call(Call),
}

impl Prim {
    /// The primitive's name as the surface spells it.
    pub fn name(&self) -> &'static str {
        match self {
            Prim::Run(_) => "run",
            Prim::Write(_) => "write",
            Prim::Remove(_) => "remove",
            Prim::Append(_) => "append",
            Prim::RegionSet(_) => "region_set",
            Prim::RegionClear(_) => "region_clear",
            Prim::Stage(_) => "stage",
            Prim::Hook(_) => "hook",
            Prim::Install(_) => "install",
            Prim::Release(_) => "release",
            Prim::Call(_) => "call",
        }
    }
}

// ---------------------------------------------------------------------------
// Builders, so terms read like the surface.

pub fn lit(s: &str) -> Value {
    Value::Lit(s.to_string())
}

pub fn param(name: &str) -> Ref {
    Ref::Param(name.to_string())
}

pub fn host_field(field: &str) -> Ref {
    Ref::Host(field.to_string())
}

pub fn fact(name: &str) -> Ref {
    Ref::Fact(name.to_string())
}

pub fn controller(name: &str) -> Ref {
    Ref::Controller(name.to_string())
}

pub fn secret(binding: &str) -> Ref {
    Ref::Secret(binding.to_string())
}

pub fn output(step: &str, name: &str, secret: bool) -> Ref {
    Ref::Output {
        step: step.to_string(),
        name: name.to_string(),
        secret,
    }
}

pub fn text(s: &str) -> Part {
    Part::Lit(s.to_string())
}

pub fn interp(r: Ref) -> Part {
    Part::Ref(r)
}

pub fn fact_ref(shape: &str) -> FactRef {
    FactRef {
        shape: shape.to_string(),
        anchor: None,
    }
}

pub fn anchored_ref(shape: &str, anchor: &str) -> FactRef {
    FactRef {
        shape: shape.to_string(),
        anchor: Some(anchor.to_string()),
    }
}

/// `run("...")` with no env and no stdin.
pub fn run(cmd: Vec<Part>) -> Prim {
    Prim::Run(Run {
        cmd,
        env: Vec::new(),
        stdin: None,
    })
}

/// `run("...")` of one literal string.
pub fn run_lit(cmd: &str) -> Prim {
    run(vec![text(cmd)])
}

pub fn write(f: FactRef, content: Value) -> Prim {
    Prim::Write(Write { fact: f, content })
}

pub fn remove(f: FactRef) -> Prim {
    Prim::Remove(Remove { fact: f })
}

pub fn append(f: FactRef, line: Value) -> Prim {
    Prim::Append(Append { fact: f, line })
}

pub fn region_set(f: FactRef, content: Value) -> Prim {
    Prim::RegionSet(RegionSet { fact: f, content })
}

pub fn region_clear(f: FactRef) -> Prim {
    Prim::RegionClear(RegionClear { fact: f })
}

pub fn stage(name: &str, content: Value, mode: u32) -> Prim {
    Prim::Stage(Stage {
        name: name.to_string(),
        content,
        mode,
    })
}

pub fn hook(name: &str, args: Vec<(&str, Value)>) -> Prim {
    Prim::Hook(Hook {
        name: name.to_string(),
        args: args
            .into_iter()
            .map(|(n, v)| KwArg {
                name: n.to_string(),
                value: v,
            })
            .collect(),
    })
}

pub fn install(name: &str) -> Prim {
    Prim::Install(Install {
        name: name.to_string(),
    })
}

pub fn release(name: &str) -> Prim {
    Prim::Release(Release {
        name: name.to_string(),
    })
}
