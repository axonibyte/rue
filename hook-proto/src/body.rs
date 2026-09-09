//! A resolved body: the instruction list an `execute.run` carries.
//!
//! The engine hands an executor — a driver of its own or a hook — a body in
//! which every value is already a string with its secrecy known, so nothing
//! downstream sees a reference or decides where a value came from. `env:`
//! and `stdin:` of a run travel on the primitive and are delivered by the
//! stdin preamble, never on a command line.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A value after resolution: its text and whether it is a secret, which
/// whoever receives it must keep off every argv, log and journal.
///
/// [`Debug`] is written by hand rather than derived: a secret's text is
/// replaced by a placeholder, so a body printed with `{:?}` -- in a log
/// line, a panic message, a failed assertion -- does not spill one. The
/// value is still reachable through the field; what is prevented is
/// spilling it without meaning to.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub text: String,
    pub secret: bool,
}

/// What a secret's text reads as when a [`Resolved`] is formatted.
pub const REDACTED: &str = "<secret>";

impl fmt::Debug for Resolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resolved")
            .field("text", &if self.secret { REDACTED } else { &self.text })
            .field("secret", &self.secret)
            .finish()
    }
}

impl Resolved {
    pub fn plain(text: &str) -> Resolved {
        Resolved {
            text: text.to_string(),
            secret: false,
        }
    }
}

/// A resolved primitive: `rue_core::body::Prim` with every value a
/// [`Resolved`]. Paths and anchors come from the primitive's fact reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RPrim {
    Run {
        cmd: Resolved,
        env: Vec<(String, Resolved)>,
        stdin: Option<Resolved>,
    },
    Write {
        shape: String,
        content: Resolved,
    },
    Remove {
        shape: String,
    },
    Append {
        shape: String,
        line: Resolved,
    },
    RegionSet {
        shape: String,
        anchor: Option<String>,
        content: Resolved,
    },
    RegionClear {
        shape: String,
        anchor: Option<String>,
    },
    Stage {
        name: String,
        content: Resolved,
        mode: u32,
    },
    Hook {
        name: String,
        args: Vec<(String, Resolved)>,
    },
    Install {
        name: String,
    },
    Release {
        name: String,
    },
    /// A defprim call, already expanded to its run template.
    Call {
        prim: String,
        cmd: Resolved,
    },
}

impl RPrim {
    /// True when any value of the primitive is a secret.
    pub fn carries_secret(&self) -> bool {
        match self {
            RPrim::Run { cmd, env, stdin } => {
                cmd.secret
                    || env.iter().any(|(_, v)| v.secret)
                    || stdin.as_ref().is_some_and(|v| v.secret)
            }
            RPrim::Write { content, .. }
            | RPrim::RegionSet { content, .. }
            | RPrim::Stage { content, .. } => content.secret,
            RPrim::Append { line, .. } => line.secret,
            RPrim::Hook { args, .. } => args.iter().any(|(_, v)| v.secret),
            RPrim::Call { cmd, .. } => cmd.secret,
            RPrim::Remove { .. }
            | RPrim::RegionClear { .. }
            | RPrim::Install { .. }
            | RPrim::Release { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_does_not_survive_being_formatted() {
        let p = RPrim::Run {
            cmd: Resolved::plain("login"),
            env: vec![(
                "PW".into(),
                Resolved {
                    text: "s3cr3t".into(),
                    secret: true,
                },
            )],
            stdin: None,
        };
        let shown = format!("{p:?}");
        assert!(!shown.contains("s3cr3t"), "{shown}");
        assert!(
            shown.contains(REDACTED) && shown.contains("login"),
            "{shown}"
        );
        // It is still on the wire, which is the whole point of the two
        // messages that may carry one.
        assert!(serde_json::to_string(&p).unwrap().contains("s3cr3t"));
        assert!(p.carries_secret());
    }
}
