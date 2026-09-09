//! The records the protocol's frames carry: what a hook registers with,
//! what a run produced, what a probe saw, what a target reports about its
//! own filesystem, and a host of an inventory (Appendix C).

use std::collections::BTreeMap;

use rue_core::model::{ArtifactLanguage, Tri};
use serde::{Deserialize, Serialize};

/// The frame a hook opens with, over the socket after `hello` or as the
/// first line of a spawned child's stdout. `protocol` must be
/// [`crate::HOOK_PROTOCOL`] (R0501 otherwise) and registration is accepted
/// only from a declared registrar (R0505).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub name: String,
    pub kinds: Vec<String>,
    pub protocol: u32,
    /// The hook serves the instance-directory ops (a run-capable host).
    #[serde(default)]
    pub filesystem: bool,
    #[serde(default)]
    pub stdin_preamble: bool,
}

/// What a body produced: the text on stdout (never journaled) and the named
/// outputs the op declared, as the executor read them back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub stdout: String,
    pub outputs: BTreeMap<String, String>,
}

/// A probe as the engine asks an executor to run it: its name (what a hook
/// knows it by) and its resolved body (what `local()` and `ssh()` run).
/// A probe's command answers a guard by its exit status: 0 yes, 1 no,
/// anything else unknown; its stdout is the fact's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeRun {
    pub name: String,
    pub body: Vec<crate::body::RPrim>,
}

/// A probe's answer: its text, and the three-valued reading a guard takes
/// (`None` reads as `Unknown`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub text: String,
    pub tri: Option<Tri>,
}

impl Observation {
    pub fn yes(text: &str) -> Observation {
        Observation {
            text: text.to_string(),
            tri: Some(Tri::Yes),
        }
    }
    pub fn no(text: &str) -> Observation {
        Observation {
            text: text.to_string(),
            tri: Some(Tri::No),
        }
    }
    pub fn unknown(text: &str) -> Observation {
        Observation {
            text: text.to_string(),
            tri: Some(Tri::Unknown),
        }
    }
    pub fn as_tri(&self) -> Tri {
        self.tri.unwrap_or(Tri::Unknown)
    }
}

/// What `rue bootstrap` verifies (section 7.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapState {
    pub rue_root: bool,
    pub group: bool,
    pub instances_dir: bool,
    pub lock: bool,
    pub modes_ok: bool,
}

impl BootstrapState {
    pub fn ready(&self) -> bool {
        self.rue_root && self.group && self.instances_dir && self.lock && self.modes_ok
    }
}

/// One instance directory as a target reports it at reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceDirState {
    pub instance: String,
    pub armed: bool,
    pub fired: bool,
    /// The directory carries the modes 7.7 requires (`2770`, group `rue`).
    /// Arming a backstop into a directory with wrong modes is R0406.
    pub modes_ok: bool,
}

/// A host as a hook lists it: the roadmap's Appendix C record.
///
/// Every field a `rue_toml()` inventory declares has a place here, so a
/// hook-listed host is the equal of a file-listed one. The three that are
/// not in Appendix C's first column -- `rue_root`, `stdin_preamble` and
/// `artifact` -- decide where the instance directory lives, whether `env:`
/// and `stdin:` may carry a secret, and what language a `:target` backstop
/// is rendered in; a hook that omits them gets the defaults below, and a
/// host without a `rue_root` cannot hold an instance directory at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryHost {
    pub name: String,
    #[serde(default)]
    pub address: String,
    pub os: String,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub reach: Vec<String>,
    #[serde(default)]
    pub filesystem: bool,
    /// Absent means "as `filesystem`": a host that can hold an instance
    /// directory can also be handed a preamble on stdin.
    #[serde(default)]
    pub stdin_preamble: Option<bool>,
    #[serde(default)]
    pub scheduler: Option<String>,
    /// Where the instance directory lives on the host (7.7). A hook that
    /// lists a run-capable host without one leaves it unable to hold one.
    #[serde(default)]
    pub rue_root: Option<String>,
    /// The language a `:target` backstop is rendered in; absent is the
    /// host's native shell.
    #[serde(default)]
    pub artifact: Option<ArtifactLanguage>,
    #[serde(default)]
    pub facts: BTreeMap<String, String>,
}
