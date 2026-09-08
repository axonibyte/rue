//! rue-render: the backstop artifact, docs/ROADMAP.md sections 5.6 and 7.7.
//!
//! A `:target` backstop is a standalone script in the instance directory
//! on the target, registered with the host's scheduler, that undoes the
//! covered steps (those with `undo_locus: :target`) in reverse order when
//! its trigger is due, with every value baked in and quoted for the family
//! that reads it. It undoes only steps whose completion marker is present,
//! removes each marker after undoing its step, applies each step's drift
//! policy as the engine would, defers a region's whole-file fallback when a
//! sibling instance's manifest holds a region on the fact, and leaves a
//! `fired` marker the engine journals on its next contact.
//!
//! The instance directory layout the artifact reads and the engine writes
//! (docs/DESIGN.md): `deadline` and `heartbeat` hold the epoch second as
//! text; `markers/<n>` holds one line `<kind> <path> <sha256>` per file
//! fact of step `n` as `do` left it; `snapshots/<n>/<k>` is the whole file
//! for entry `k` of step `n`; `manifest` lists `region <path> <anchor>`
//! lines; `fired`, `drift` and `clobbered` are written by the artifact.
//!
//! Three languages over one action list: POSIX `sh`, PowerShell, and
//! Python run by `uv` with PEP 723 metadata, on any OS. This crate depends
//! only on core and performs no I/O.

pub mod actions;
pub mod quote;
mod template;

use std::fmt;

use rue_core::algebra::{numbered, op_of};
use rue_core::artifact::{language_of, shell_of, supported, Shell};
use rue_core::backstop::coverage;
use rue_core::diagnostics::Code;
use rue_core::model::{ArtifactLanguage, HostRecord, Plan, Site, Trigger};

pub use actions::{Action, Bindings, FileFact, Step};
pub use quote::{Family, Unquotable};

/// The shell helpers the `sh` artifact carries (region markers, digests,
/// atomic restore): the `ssh()` executor runs the same text on a target,
/// so the engine and the artifact strip and restore by one rule.
pub fn sh_helpers() -> &'static str {
    template::sh::HELPERS
}

/// Where the artifact lives on the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub id: String,
    /// `<rue_root>`; `None` is the family's default (section 4.5).
    pub rue_root: Option<String>,
}

impl Instance {
    /// The default `rue_root` of a shell family.
    pub fn default_root(shell: Shell) -> &'static str {
        match shell {
            Shell::Posix => "/var/db/rue",
            Shell::Powershell => "C:\\ProgramData\\rue",
        }
    }

    pub fn root(&self, shell: Shell) -> String {
        self.rue_root
            .clone()
            .unwrap_or_else(|| Self::default_root(shell).to_string())
    }
}

/// The rendered artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub language: ArtifactLanguage,
    pub file_name: &'static str,
    pub text: String,
}

/// Why a render was refused. The two with a code are diagnostics; the rest
/// are contract errors of the render call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// The plan has no backstop.
    NoBackstop,
    /// The backstop covers no step (no `:target` undo).
    NotTarget,
    /// The host is not in the site.
    UnknownHost(String),
    /// No template for the host's os and artifact language (E0403).
    Unsupported {
        os: String,
        language: ArtifactLanguage,
    },
    /// A value cannot be quoted for its family (E0109).
    Unquotable { step: u32, inner: Unquotable },
    /// Something in a covered undo an artifact cannot carry out.
    NotBakeable { step: u32, what: String },
    /// A parameter or host field the bindings do not supply.
    Unbound { step: u32, name: String },
    /// A fact shape an artifact cannot observe: not a static `file:` path.
    Unobservable { step: u32, shape: String },
}

impl RenderError {
    pub fn code(&self) -> Option<Code> {
        match self {
            RenderError::Unsupported { .. } => Some(Code::E0403),
            RenderError::Unquotable { .. } => Some(Code::E0109),
            _ => None,
        }
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::NoBackstop => write!(f, "the plan has no backstop"),
            RenderError::NotTarget => write!(f, "the backstop covers no step: no :target undo"),
            RenderError::UnknownHost(h) => write!(f, "host {h} is not in the site"),
            RenderError::Unsupported { os, language } => {
                write!(
                    f,
                    "{}: no {} template for os {os}",
                    Code::E0403,
                    language.name()
                )
            }
            RenderError::Unquotable { step, inner } => {
                write!(f, "{} at step {step}: {inner}", Code::E0109)
            }
            RenderError::NotBakeable { step, what } => {
                write!(f, "step {step}: not bakeable into an artifact: {what}")
            }
            RenderError::Unbound { step, name } => {
                write!(f, "step {step}: {name} is not bound")
            }
            RenderError::Unobservable { step, shape } => write!(
                f,
                "step {step}: {shape} is not a static file: shape an artifact can observe"
            ),
        }
    }
}

impl std::error::Error for RenderError {}

/// Everything a template needs, computed once.
pub(crate) struct Context<'a> {
    pub plan: &'a Plan,
    pub host: &'a HostRecord,
    pub shell: Shell,
    pub language: ArtifactLanguage,
    pub instance: &'a Instance,
    pub root: String,
    /// A deadline file is compared when any `after:` or `unless_confirmed:`
    /// trigger is declared.
    pub deadline: bool,
    /// The heartbeat deadline in seconds, when `unless_heartbeat:` is declared.
    pub heartbeat_s: Option<u64>,
    /// Covered steps in reverse order: the artifact's undo order.
    pub steps: Vec<Step>,
}

/// Render the artifact for `host`'s record, or say why not.
pub fn render(
    site: &Site,
    plan: &Plan,
    host: &str,
    instance: &Instance,
    bindings: &Bindings,
) -> Result<Artifact, RenderError> {
    let b = plan.backstop.as_ref().ok_or(RenderError::NoBackstop)?;
    let cov = coverage(plan).ok_or(RenderError::NoBackstop)?;
    if cov.covered.is_empty() {
        return Err(RenderError::NotTarget);
    }
    let h = site
        .hosts
        .iter()
        .find(|r| r.name == host)
        .ok_or_else(|| RenderError::UnknownHost(host.to_string()))?;
    let shell = shell_of(&h.os);
    let language = language_of(h);
    if !supported(&h.os, language) {
        return Err(RenderError::Unsupported {
            os: h.os.clone(),
            language,
        });
    }
    let numbered_items = numbered(&plan.body);
    let mut steps = Vec::new();
    for n in cov.covered.iter().rev() {
        let op = numbered_items
            .iter()
            .find(|(m, _)| m == n)
            .and_then(|(_, it)| op_of(it))
            .expect("a covered step is a numbered op");
        steps.push(actions::step_actions(*n, op, shell, h, bindings)?);
    }
    let deadline = b
        .triggers
        .iter()
        .any(|t| matches!(t, Trigger::After(_) | Trigger::UnlessConfirmed(_)));
    let heartbeat_s = b.triggers.iter().find_map(|t| match t {
        Trigger::UnlessHeartbeat { deadline, .. } => Some(deadline.seconds),
        _ => None,
    });
    let ctx = Context {
        plan,
        host: h,
        shell,
        language,
        instance,
        root: instance.root(shell),
        deadline,
        heartbeat_s,
        steps,
    };
    let (file_name, text) = match language {
        ArtifactLanguage::Sh => ("artifact.sh", template::sh::render(&ctx)?),
        ArtifactLanguage::Powershell => ("artifact.ps1", template::powershell::render(&ctx)?),
        ArtifactLanguage::Python => ("artifact.py", template::python::render(&ctx)?),
    };
    Ok(Artifact {
        language,
        file_name,
        text,
    })
}
