//! Every op of docs/ROADMAP.md 7.5 as data.
//!
//! [`OPS`] is the one enumeration of the protocol's surface: the eight
//! kinds, the ops of each, the fields a request carries, the fields a reply
//! must carry for the engine to accept it (R0303 otherwise), the fields it
//! may carry, and whether the op is one of the four messages a secret
//! travels in. The engine's R0305 guard, the SDKs' dispatch tables,
//! `rue sdk-conform`'s cases and the frozen-protocol golden all read this
//! table rather than repeating it, so a change to the wire is a change to
//! one array.

/// The shell a hook's command is run through, and the flag that hands it
/// one command: the host's own, since `--spawn NAME=COMMAND` is written in
/// whatever the operator's machine speaks. `rued` runs on all three
/// families (7.4), so hard-coding `sh` made a spawned hook a thing only
/// two of them could have.
pub fn host_shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    }
}

/// The version a [`crate::Registration`] must name; R0501 otherwise.
pub const HOOK_PROTOCOL: u32 = 1;

/// Which way a secret travels in the op that may carry one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// In the request, from the engine to the hook.
    ToHook,
    /// In the reply, from the hook to the engine.
    ToEngine,
}

/// One op of the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Op {
    pub kind: &'static str,
    pub op: &'static str,
    /// The fields the engine puts in the request.
    pub request: &'static [&'static str],
    /// The fields an `ok: true` reply must carry; a reply without one is
    /// R0303, a contract violation the engine treats as a refusal.
    pub required_reply: &'static [&'static str],
    /// Fields a reply may carry; their absence is not a violation.
    pub optional_reply: &'static [&'static str],
    /// `Some` for the four messages of 7.5 that may carry a secret, and the
    /// direction it travels in. Every other message is scrubbed (R0305).
    pub secret: Option<Direction>,
    /// The hook may answer `ok: false` to say it does not serve this op at
    /// all, and the engine records the absence rather than refusing. Only
    /// `execute.clock` is optional in that sense (docs/hook-protocol.md).
    pub optional: bool,
}

impl Op {
    /// The op by kind and name, or `None` for a pair the protocol has no
    /// row for.
    pub fn find(kind: &str, op: &str) -> Option<&'static Op> {
        OPS.iter().find(|o| o.kind == kind && o.op == op)
    }

    /// True for the messages a secret may travel in; the engine's R0305
    /// guard drops every secret from any other request.
    pub fn may_carry_secret(&self) -> bool {
        self.secret.is_some()
    }
}

/// The eight kinds a hook may register, in the order 7.5 lists them.
pub const KINDS: &[&str] = &[
    "journal",
    "inventory",
    "execute",
    "probe",
    "approval",
    "secrets",
    "notify",
    "scheduler",
];

/// Every op, in the order docs/hook-protocol.md tabulates them.
pub const OPS: &[Op] = &[
    Op {
        kind: "journal",
        op: "append",
        request: &["entry"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "inventory",
        op: "list",
        request: &[],
        required_reply: &["hosts"],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "run",
        request: &["host", "instance", "body", "env", "secrets"],
        required_reply: &["output"],
        optional_reply: &["facts"],
        // Toward the hook in the body; toward the engine in the outputs an
        // op declared secret. One row, both halves of the same message.
        secret: Some(Direction::ToHook),
        optional: false,
    },
    Op {
        kind: "execute",
        op: "read_fact",
        request: &["host", "shape"],
        // An absent `content` is the answer "no such file", not a violation.
        required_reply: &[],
        optional_reply: &["content"],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "bootstrap_state",
        request: &["host"],
        required_reply: &["state"],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "clock",
        request: &["host"],
        required_reply: &["epoch_s"],
        optional_reply: &[],
        secret: None,
        optional: true,
    },
    Op {
        kind: "execute",
        op: "instance_dir_create",
        request: &["host", "instance"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "instance_dir_remove",
        request: &["host", "instance"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "instance_dir_list",
        request: &["host"],
        required_reply: &["dirs"],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "put_file",
        request: &["host", "instance", "rel", "content", "mode"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "replace_file",
        request: &["host", "instance", "rel", "content"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "get_file",
        request: &["host", "instance", "rel"],
        required_reply: &["content"],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "remove_file",
        request: &["host", "instance", "rel"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "execute",
        op: "host_lock",
        request: &["host"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "probe",
        op: "observe",
        request: &["host", "probe"],
        required_reply: &["fact"],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "approval",
        op: "authenticators",
        request: &[],
        required_reply: &["authenticators"],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "approval",
        op: "challenge",
        request: &["instance", "digest", "scope", "context"],
        required_reply: &["challenge"],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "approval",
        op: "verify",
        request: &["instance", "digest", "scope", "authenticator", "proof"],
        required_reply: &["verified"],
        optional_reply: &["reason"],
        secret: None,
        optional: false,
    },
    Op {
        kind: "secrets",
        op: "resolve",
        request: &["ref"],
        required_reply: &["value"],
        optional_reply: &[],
        secret: Some(Direction::ToEngine),
        optional: false,
    },
    Op {
        kind: "secrets",
        op: "deliver",
        request: &["instance", "label", "value"],
        required_reply: &["accepted"],
        optional_reply: &["receipt"],
        secret: Some(Direction::ToHook),
        optional: false,
    },
    Op {
        kind: "notify",
        op: "deliver",
        request: &["level", "subject", "body"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "scheduler",
        op: "install",
        request: &["host", "artifact", "deadline"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "scheduler",
        op: "arm",
        request: &["host", "artifact", "deadline"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "scheduler",
        op: "rearm",
        request: &["host", "artifact", "deadline"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "scheduler",
        op: "disarm",
        request: &["host", "artifact", "deadline"],
        required_reply: &[],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
    Op {
        kind: "scheduler",
        op: "present",
        request: &["host", "artifact", "deadline"],
        required_reply: &["present"],
        optional_reply: &[],
        secret: None,
        optional: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// `rued --spawn`, `rue sdk-conform` and the `rue-hook` shim all hand
    /// one string to a shell, and which shell that is follows the host. A
    /// hook spawned on Windows was unreachable while this said `sh`
    /// everywhere, and the failure it produced -- a hook that will not
    /// start -- reads as the hook's fault.
    #[test]
    fn the_shell_a_hook_is_spawned_through_is_the_host_s_own() {
        let (shell, flag) = host_shell();
        if cfg!(windows) {
            assert_eq!((shell, flag), ("cmd", "/C"));
        } else {
            assert_eq!((shell, flag), ("sh", "-c"));
        }
    }

    #[test]
    fn every_op_is_named_once_and_belongs_to_a_declared_kind() {
        let mut seen: Vec<(&str, &str)> = OPS.iter().map(|o| (o.kind, o.op)).collect();
        let n = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), n, "an op is tabulated twice");
        for o in OPS {
            assert!(KINDS.contains(&o.kind), "{} is not a declared kind", o.kind);
            assert!(Op::find(o.kind, o.op).is_some());
        }
        for k in KINDS {
            assert!(
                OPS.iter().any(|o| &o.kind == k),
                "kind {k} has no op, so nothing can register for it"
            );
        }
        assert!(Op::find("execute", "reboot").is_none());
    }

    #[test]
    fn exactly_four_messages_may_carry_a_secret() {
        let carrying: Vec<(&str, &str)> = OPS
            .iter()
            .filter(|o| o.may_carry_secret())
            .map(|o| (o.kind, o.op))
            .collect();
        // 7.5: toward the hook in `execute.run`'s body and `secrets.deliver`;
        // toward the engine in `execute.run`'s outputs and `secrets.resolve`.
        // `execute.run` is one row for both of its halves, so three rows
        // carry the four messages.
        assert_eq!(
            carrying,
            vec![
                ("execute", "run"),
                ("secrets", "resolve"),
                ("secrets", "deliver")
            ]
        );
    }

    #[test]
    fn a_required_field_is_never_also_optional() {
        for o in OPS {
            for f in o.required_reply {
                assert!(
                    !o.optional_reply.contains(f),
                    "{}.{}: `{f}` is both required and optional",
                    o.kind,
                    o.op
                );
            }
        }
    }

    #[test]
    fn only_the_clock_may_be_declined_by_an_ok_false() {
        let optional: Vec<&str> = OPS.iter().filter(|o| o.optional).map(|o| o.op).collect();
        assert_eq!(optional, vec!["clock"]);
    }
}
