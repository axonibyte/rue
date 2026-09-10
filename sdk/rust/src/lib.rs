//! The reference embedding SDK, docs/ROADMAP.md 7.11.
//!
//! Any language that can open a socket or a pipe and speak newline-delimited
//! JSON can embed rue; this crate is the convenience over that protocol, and
//! the shape every other SDK in `sdk/` is written to match. It gives you:
//!
//! * a trait per kind ([`Journal`], [`Inventory`], [`Execute`], [`Probe`],
//!   [`Approval`], [`Secrets`], [`Notify`], [`Scheduler`]), each method
//!   typed in the records of `rue-hook-proto` rather than in `Value`;
//! * [`Hooks`], which collects the kinds you actually implement and builds
//!   the registration frame from them, so a hook cannot register for a kind
//!   it does not serve;
//! * [`serve_stdio`] and [`serve_socket`], the two ways `rued` reaches a
//!   hook (docs/hook-protocol.md), including the registration handshake;
//! * the secret rule: an `execute.run` hands its handler the resolved body
//!   with its secrets intact, and nothing else does. A [`Resolved`] whose
//!   `secret` is set formats as a placeholder, so a body that reaches a log
//!   line or a panic message does not carry the value with it.
//!
//! ## What the engine expects of you
//!
//! One reply per request, on one line, carrying the id it came with. An
//! `ok: true` reply must carry every field its op declares
//! ([`rue_hook_proto::Op::required_reply`]) or the engine refuses the step
//! with R0303 -- this crate builds those replies for you, which is most of
//! why it exists. An op you do not serve is `ok: false`, and a refusal is
//! honest: it names a reason the operator will read.
//!
//! No reply at all is **Silent**, which the engine treats as a refusal of
//! the step with nothing to say about why. The deadline is the engine's
//! (`rued run --hook-deadline`) and is not on the wire, so an SDK cannot
//! see it; what it can do is keep your own slowness from turning into
//! silence. Set [`ServeOptions::budget`] and a handler that overruns it
//! answers `ok: false` naming the overrun instead of leaving the engine to
//! time out -- a refusal with a reason beats a silence, whenever the hook
//! is merely slow rather than gone.

use std::io::{BufRead, Write};

use rue_hook_proto::{
    BootstrapState, InstanceDirState, InventoryHost, Observation, Op, Output, ProbeRun, RPrim,
    Registration, Resolved, HOOK_PROTOCOL,
};
use serde_json::{json, Value};

pub use rue_hook_proto as proto;

mod hooks;
mod serve;

pub use hooks::{Hooks, Presence};
pub use serve::{serve_socket, serve_stdio, ServeOptions};

/// Why a hook will not answer an op: the text the engine journals and the
/// operator reads. A refusal is not an error in this crate's sense -- it is
/// a hook saying no, on the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl Refusal {
    pub fn new(why: impl Into<String>) -> Refusal {
        Refusal(why.into())
    }

    /// The refusal for an op this hook does not implement. `execute.clock`
    /// is the one op the engine reads this as an absence rather than a
    /// fault (docs/hook-protocol.md).
    pub fn unserved(kind: &str, op: &str) -> Refusal {
        Refusal(format!("this hook does not serve {kind}.{op}"))
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refusal {}

/// A result that is either an answer or a refusal the engine will journal.
pub type Answer<T> = Result<T, Refusal>;

/// `journal to: hook(:name)`. A sink that does not acknowledge is R0304 at
/// the journal and the plan does not proceed, so refuse only when you truly
/// have not recorded the entry.
pub trait Journal: Send {
    fn append(&mut self, entry: &Value) -> Answer<()>;
}

/// `inventory from: hook(:name)`. The hosts of the site, as Appendix C
/// records; the engine asks once, as it starts.
pub trait Inventory: Send {
    fn list(&mut self) -> Answer<Vec<InventoryHost>>;
}

/// `execute via: hook(:name, transport: :t)`. The instance-directory ops
/// below `run` are required only of a hook that registers `filesystem`
/// (7.7); a hook without one is never asked them.
pub trait Execute: Send {
    /// The one message that carries secrets toward a hook, with
    /// `secrets[i]` true for each primitive of `body` that holds one. A
    /// secret must not reach a command line, a log or a journal on your
    /// side; put it on the child's stdin or in a mode-0600 staged file.
    fn run(&mut self, host: &str, instance: &str, body: &[RPrim]) -> Answer<Output>;

    /// The file's text, or `None` for no such file -- which is an answer,
    /// not a refusal.
    fn read_fact(&mut self, host: &str, shape: &str) -> Answer<Option<String>> {
        let _ = (host, shape);
        Err(Refusal::unserved("execute", "read_fact"))
    }

    fn bootstrap_state(&mut self, host: &str) -> Answer<BootstrapState> {
        let _ = host;
        Err(Refusal::unserved("execute", "bootstrap_state"))
    }

    /// The host's own clock in epoch seconds, for the skew probe an arm
    /// makes (R0403). The one op whose refusal the engine reads as "no
    /// skew probe is possible here" rather than as a fault.
    fn clock(&mut self, host: &str) -> Answer<u64> {
        let _ = host;
        Err(Refusal::unserved("execute", "clock"))
    }

    fn instance_dir_create(&mut self, host: &str, instance: &str) -> Answer<()> {
        let _ = (host, instance);
        Err(Refusal::unserved("execute", "instance_dir_create"))
    }

    fn instance_dir_remove(&mut self, host: &str, instance: &str) -> Answer<()> {
        let _ = (host, instance);
        Err(Refusal::unserved("execute", "instance_dir_remove"))
    }

    fn instance_dir_list(&mut self, host: &str) -> Answer<Vec<InstanceDirState>> {
        let _ = host;
        Err(Refusal::unserved("execute", "instance_dir_list"))
    }

    fn put_file(
        &mut self,
        host: &str,
        instance: &str,
        rel: &str,
        content: &str,
        mode: u32,
    ) -> Answer<()> {
        let _ = (host, instance, rel, content, mode);
        Err(Refusal::unserved("execute", "put_file"))
    }

    fn replace_file(&mut self, host: &str, instance: &str, rel: &str, content: &str) -> Answer<()> {
        let _ = (host, instance, rel, content);
        Err(Refusal::unserved("execute", "replace_file"))
    }

    fn get_file(&mut self, host: &str, instance: &str, rel: &str) -> Answer<String> {
        let _ = (host, instance, rel);
        Err(Refusal::unserved("execute", "get_file"))
    }

    fn remove_file(&mut self, host: &str, instance: &str, rel: &str) -> Answer<()> {
        let _ = (host, instance, rel);
        Err(Refusal::unserved("execute", "remove_file"))
    }

    /// Held until the next request on the host. Lock order is host, then
    /// instance, always (7.7).
    fn host_lock(&mut self, host: &str) -> Answer<()> {
        let _ = host;
        Err(Refusal::unserved("execute", "host_lock"))
    }
}

/// `probe` on the same registration as `execute`: a guard's three-valued
/// answer and the fact's text.
pub trait Probe: Send {
    fn observe(&mut self, host: &str, probe: &str) -> Answer<Observation>;
}

/// `approval via: hook(:name)`. The digest and its scope are rue's, so a
/// proof you accept is bound to one request and one scope: verify against
/// the digest you were handed and never against a request you rebuilt.
pub trait Approval: Send {
    fn authenticators(&mut self) -> Answer<Vec<Authenticator>>;
    fn challenge(&mut self, r: &ChallengeRequest) -> Answer<String>;
    fn verify(&mut self, r: &VerifyRequest) -> Answer<Verdict>;
}

/// An authenticator the approval hook publishes; `human` decides whether it
/// can satisfy a gate that requires a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authenticator {
    pub id: String,
    pub human: bool,
}

/// What the engine asks you to render for a person to approve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeRequest {
    pub instance: String,
    /// The request digest, hex. It is the identity of this approval.
    pub digest: String,
    /// `plan`, `step` with its number, or `ack` with its number (5.11).
    pub scope: Value,
    pub context: Value,
}

/// A proof offered against one digest and one scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyRequest {
    pub instance: String,
    pub digest: String,
    pub scope: Value,
    pub authenticator: String,
    pub proof: String,
}

/// The answer to a [`VerifyRequest`]: whether it verified and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub verified: bool,
    pub reason: String,
}

/// `secrets from:` and `secrets deliver_to: hook(:name)`. Both of these
/// carry a secret -- `resolve`'s answer toward the engine, `deliver`'s
/// value toward you -- and they are two of the four messages in the
/// protocol that may.
pub trait Secrets: Send {
    /// The value behind a `secret(:ref)`.
    fn resolve(&mut self, reference: &str) -> Answer<String> {
        let _ = reference;
        Err(Refusal::unserved("secrets", "resolve"))
    }

    /// Take delivery of a secret an op produced. Answer `false` to decline
    /// it, and the engine offers it to the next acceptor; the receipt is
    /// what it journals in place of the value.
    fn deliver(&mut self, instance: &str, label: &str, value: &str) -> Answer<Delivery> {
        let _ = (instance, label, value);
        Err(Refusal::unserved("secrets", "deliver"))
    }
}

/// Whether a secret was accepted, and the receipt to journal instead of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub accepted: bool,
    pub receipt: String,
}

/// `notify via: hook(:name)`: an unbounded state, re-sent each reap pass.
pub trait Notify: Send {
    fn deliver(&mut self, level: &str, subject: &str, body: &str) -> Answer<()>;
}

/// `backstop scheduler: hook(:name)`. The engine owns the instance
/// directory; these five carry the target-side entry that runs a rendered
/// artifact. `arm` and `rearm` carry the deadline for a scheduler that
/// enforces the time itself; one whose entry is periodic has nothing to do
/// in them. Answer `present` with [`Presence::Unknown`] rather than
/// guessing: the engine never reads unknown as absence.
pub trait Scheduler: Send {
    fn install(&mut self, host: &str, artifact: &str) -> Answer<()>;
    fn arm(&mut self, host: &str, artifact: &str, deadline: Option<u64>) -> Answer<()>;
    fn rearm(&mut self, host: &str, artifact: &str, deadline: Option<u64>) -> Answer<()>;
    fn disarm(&mut self, host: &str, artifact: &str) -> Answer<()>;
    fn present(&mut self, host: &str, artifact: &str) -> Answer<Presence>;
}

/// A probe run as the engine asks for it. `local()` and `ssh()` execute the
/// body; a hook is given the name it knows the probe by, and the body with
/// it for a hook that would rather run it.
pub type Probed = ProbeRun;

/// The registration frame this hook opens with, built from the kinds it
/// actually serves.
pub fn registration(name: &str, hooks: &Hooks) -> Registration {
    Registration {
        name: name.to_string(),
        kinds: hooks.kinds(),
        protocol: HOOK_PROTOCOL,
        filesystem: hooks.filesystem,
        stdin_preamble: hooks.stdin_preamble,
    }
}

/// Whether any value of a resolved body is a secret; the handler for an
/// `execute.run` uses it to decide where the body may be written.
pub fn carries_secret(body: &[RPrim]) -> bool {
    body.iter().any(RPrim::carries_secret)
}

/// The text of a resolved value. Named rather than reached through the
/// field so that a use of a secret is a visible act in the code that
/// makes it.
pub fn expose(v: &Resolved) -> &str {
    &v.text
}

/// The reply frame for a refusal.
pub fn refusal_frame(id: Value, why: &Refusal) -> Value {
    json!({ "id": id, "ok": false, "error": why.0 })
}

/// The reply frame for an answer, with the op's fields merged in. The op
/// is looked up so that a reply this crate builds cannot omit a required
/// field: an op whose fields are missing here is a bug in this crate, not
/// an R0303 the user has to debug at the far end.
pub fn ok_frame(id: Value, op: &Op, fields: Value) -> Value {
    let mut v = json!({ "id": id, "ok": true });
    if let Value::Object(m) = fields {
        for (k, val) in m {
            v[k] = val;
        }
    }
    debug_assert!(
        op.required_reply.iter().all(|f| v.get(*f).is_some()),
        "{}.{} reply is missing a required field: {v}",
        op.kind,
        op.op
    );
    v
}

/// Read one line and parse it as a frame; `None` at end of input.
///
/// Public because a hook that must answer off the SDK's rails -- the
/// conformance hook's deliberate provocations are the only honest example
/// -- needs the same framing as the serve loop rather than a second one.
pub fn read_frame<R: BufRead>(r: &mut R) -> std::io::Result<Option<Value>> {
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if line.trim().is_empty() {
        return Ok(Some(Value::Null));
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Write one frame as a line and flush it: an unflushed reply is a silence.
pub fn write_frame<W: Write>(w: &mut W, v: &Value) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(v)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}
