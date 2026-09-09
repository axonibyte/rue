//! The hook protocol, docs/ROADMAP.md 7.5 and docs/hook-protocol.md v1:
//! newline-delimited JSON between the engine and a registered hook, over
//! the control socket after `hello` and `register`, or over the stdio of a
//! child the daemon spawned. Every request carries an id and a deadline; a
//! hook that misses the deadline is `Silent`; a hook that answers `ok:
//! true` without a field the op requires is R0303.
//!
//! A [`HookLink`] is one connection's request/reply channel; the
//! [`HookRegistry`] maps registered names to links; the adapters below
//! (`HookExecutor`, `HookSink`, `HookInventory`) present a hook of a kind
//! as the engine's own trait for that kind, looking the link up by name at
//! call time, so a plan whose hook is not registered yet refuses honestly
//! (Silent for an executor, R0304 for a sink) rather than the daemon
//! refusing to start.
//!
//! Secrets cross this boundary in exactly four messages (7.5): toward the
//! hook in `execute.run`'s body and `secrets.deliver`, toward the engine
//! in `execute.run`'s outputs and `secrets.resolve`. The constructors here
//! take secret-bearing values only for those; no other message has a place
//! to put one.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rue_core::journal::Entry;
use rue_core::journal::Scope;
use rue_core::model::{Authenticator, HostRecord, Instant, Tri};
use rue_hook_proto::{Direction, InventoryHost, Op};
use serde_json::{json, Value};

use crate::executor::{
    BootstrapState, ExecCaps, ExecError, Executor, HostLockGuard, InstanceDirState, LocusKind,
    Observation, Output, ProbeRun, RPrim,
};
use crate::gates::{hex, Approval, ProofRequest, Verified};
use crate::host::Host;
use crate::journal::Sink;
use crate::notify::{Level, Notify};
use crate::scheduler::{Job, Presence, Scheduler};
use crate::secrets::Acceptor;

pub use rue_hook_proto::{Registration, HOOK_PROTOCOL};

/// The default deadline a hook has to answer a request.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookError {
    /// No answer within the deadline, or the connection is gone.
    Silent,
    /// `ok: false` with the hook's reason.
    Refused(String),
    /// R0303: `ok: true` without a field the op requires.
    Contract(String),
    /// No hook of that name is registered.
    Unregistered(String),
    Io(String),
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookError::Silent => write!(f, "hook silent: no answer within the deadline"),
            HookError::Refused(r) => write!(f, "hook refused: {r}"),
            HookError::Contract(m) => write!(f, "R0303: hook contract violation: {m}"),
            HookError::Unregistered(n) => write!(f, "hook {n} is not registered"),
            HookError::Io(e) => write!(f, "hook i/o: {e}"),
        }
    }
}

impl std::error::Error for HookError {}

/// A request/reply channel to one hook.
pub trait HookLink: Send + Sync {
    fn name(&self) -> String;
    fn call(&self, request: Value, deadline: Duration) -> Result<Value, HookError>;
}

/// One connection's link: requests written as lines to `writer`, replies
/// routed by id to the waiting caller by whoever reads the other direction
/// (`deliver`).
pub struct LineLink {
    name: String,
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    next_id: AtomicU64,
}

impl fmt::Debug for LineLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LineLink({})", self.name)
    }
}

impl LineLink {
    pub fn new(name: &str, writer: Box<dyn Write + Send>) -> LineLink {
        LineLink {
            name: name.to_string(),
            writer: Mutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// A reply line arrived on the connection: hand it to its caller. A
    /// reply nobody waits for (late, or unknown) is dropped.
    pub fn deliver(&self, reply: Value) {
        let id = reply.get("id").and_then(Value::as_u64);
        if let Some(id) = id {
            let tx = self
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            if let Some(tx) = tx {
                let _ = tx.send(reply);
            }
        }
    }

    /// Write raw bytes on the link (the registration acknowledgement a
    /// child reads on its stdin).
    pub fn call_raw_write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        w.write_all(bytes)?;
        w.flush()
    }

    /// Read lines from `reader` until it ends, delivering each; for a child
    /// hook's stdout, on its own thread.
    pub fn pump(self: &Arc<Self>, mut reader: Box<dyn BufRead + Send>) {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        self.deliver(v);
                    }
                }
            }
        }
        // The connection is gone: every waiter learns it by the dropped
        // sender.
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

/// Whether a secret may travel toward the hook in this request: the body of
/// `execute.run` and the value of `secrets.deliver` (7.5). The other two of
/// the four travel toward the engine, in replies. The rule is the wire's,
/// so it is read from the table and not repeated here.
fn permitted(request: &Value) -> bool {
    let kind = request.get("kind").and_then(Value::as_str).unwrap_or("");
    let op = request.get("op").and_then(Value::as_str).unwrap_or("");
    Op::find(kind, op).is_some_and(|o| o.secret == Some(Direction::ToHook))
}

/// What a secret looks like once resolved: an object with a `text` and
/// `secret: true`. In a message that may not carry one, the text goes and
/// the label stays, so the journal and the hook both see that something
/// was dropped and neither sees the value.
pub const DROPPED: &str = "<secret dropped: R0305>";

fn scrub(v: &mut Value, path: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            let is_secret = m.get("secret").and_then(Value::as_bool) == Some(true)
                && m.get("text").is_some_and(Value::is_string);
            if is_secret {
                m.insert("text".into(), json!(DROPPED));
                out.push(if path.is_empty() {
                    "a value".to_string()
                } else {
                    path.to_string()
                });
                return;
            }
            let keys: Vec<String> = m.keys().cloned().collect();
            for k in keys {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                if let Some(x) = m.get_mut(&k) {
                    scrub(x, &child, out);
                }
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter_mut().enumerate() {
                scrub(x, &format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}

/// Drop every secret from a request that may not carry one; the labels
/// dropped, empty when the message is permitted or carries none.
pub fn guard_secrets(request: &mut Value) -> Vec<String> {
    if permitted(request) {
        return Vec::new();
    }
    let mut out = Vec::new();
    scrub(request, "", &mut out);
    out
}

impl HookLink for LineLink {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn call(&self, mut request: Value, deadline: Duration) -> Result<Value, HookError> {
        // R0305: a secret travels toward a hook in exactly two messages
        // (5.13, 7.5). In any other, the value is dropped here, at the
        // seam, before a line is written.
        let dropped = guard_secrets(&mut request);
        if !dropped.is_empty() {
            eprintln!(
                "rue: R0305: {} dropped from a {} message to hook {}",
                dropped.join(", "),
                request
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("(unknown)"),
                self.name
            );
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        request["id"] = json!(id);
        let (tx, rx): (Sender<Value>, Receiver<Value>) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        {
            let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
            let mut line =
                serde_json::to_vec(&request).map_err(|e| HookError::Io(e.to_string()))?;
            line.push(b'\n');
            if let Err(e) = w.write_all(&line).and_then(|()| w.flush()) {
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                return Err(HookError::Io(e.to_string()));
            }
        }
        match rx.recv_timeout(deadline) {
            Ok(reply) => reply_of(reply),
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                Err(HookError::Silent)
            }
        }
    }
}

/// `ok: true` yields the reply; `ok: false` its error; anything else is a
/// contract violation.
fn reply_of(reply: Value) -> Result<Value, HookError> {
    match reply.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(reply),
        Some(false) => Err(HookError::Refused(
            reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("no reason given")
                .to_string(),
        )),
        None => Err(HookError::Contract("reply carries no boolean `ok`".into())),
    }
}

/// A required field of a reply, or R0303.
pub fn field<'a>(reply: &'a Value, name: &str) -> Result<&'a Value, HookError> {
    reply
        .get(name)
        .ok_or_else(|| HookError::Contract(format!("reply lacks `{name}`")))
}

// ---------------------------------------------------------------------------
// Registration

pub struct Registered {
    pub registration: Registration,
    pub registrar: String,
    pub connection: String,
    pub link: Arc<dyn HookLink>,
}

#[derive(Default)]
pub struct HookRegistry {
    hooks: Mutex<BTreeMap<String, Registered>>,
}

impl fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HookRegistry({:?})", self.names())
    }
}

impl HookRegistry {
    pub fn new() -> HookRegistry {
        HookRegistry::default()
    }

    pub fn register(&self, r: Registered) {
        self.hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(r.registration.name.clone(), r);
    }

    pub fn deregister(&self, name: &str) -> Option<(String, String)> {
        self.hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name)
            .map(|r| (r.registrar, r.connection))
    }

    pub fn names(&self) -> Vec<String> {
        self.hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    pub fn link(&self, name: &str) -> Result<(Arc<dyn HookLink>, Registration), HookError> {
        self.hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .map(|r| (r.link.clone(), r.registration.clone()))
            .ok_or_else(|| HookError::Unregistered(name.to_string()))
    }

    /// Every registered hook serving a kind.
    pub fn serving(&self, kind: &str) -> Vec<String> {
        self.hooks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|r| r.registration.kinds.iter().any(|k| k == kind))
            .map(|r| r.registration.name.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Requests

// One constructor per op of 7.5, in `rue-hook-proto` so the engine, the
// SDKs and the conformance runner build the same frames from the same
// table.
pub use rue_hook_proto::request::*;

// ---------------------------------------------------------------------------
// Adapters

/// `execute via: hook(:name, transport: :t)`: the hook as an executor.
pub struct HookExecutor {
    pub name: String,
    pub transport: String,
    pub registry: Arc<HookRegistry>,
    pub deadline: Duration,
}

impl HookExecutor {
    fn call(&self, request: Value) -> Result<Value, ExecError> {
        let (link, _) = self
            .registry
            .link(&self.name)
            .map_err(|e| ExecError::Unreachable(e.to_string()))?;
        link.call(request, self.deadline).map_err(|e| match e {
            HookError::Silent => ExecError::Silent,
            HookError::Refused(r) => ExecError::Failed(r),
            HookError::Contract(m) => ExecError::Failed(format!("R0303: {m}")),
            HookError::Unregistered(n) => {
                ExecError::Unreachable(format!("hook {n} not registered"))
            }
            HookError::Io(m) => ExecError::Io(m),
        })
    }
}

impl Executor for HookExecutor {
    fn locus(&self) -> LocusKind {
        LocusKind::Hook(self.transport.clone())
    }

    fn capabilities(&self) -> ExecCaps {
        match self.registry.link(&self.name) {
            Ok((_, r)) => ExecCaps {
                filesystem: r.filesystem,
                stdin_preamble: r.stdin_preamble,
            },
            Err(_) => ExecCaps {
                filesystem: false,
                stdin_preamble: false,
            },
        }
    }

    fn run(&mut self, host: &Host, instance: &str, body: &[RPrim]) -> Result<Output, ExecError> {
        let reply = self.call(execute_run(host.name(), instance, body))?;
        let out = field(&reply, "output").map_err(|e| ExecError::Failed(e.to_string()))?;
        let stdout = out
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let outputs = out
            .get("outputs")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Output { stdout, outputs })
    }

    fn observe(&mut self, host: &Host, probe: &ProbeRun) -> Result<Observation, ExecError> {
        let reply = self.call(probe_observe(host.name(), &probe.name))?;
        let fact = field(&reply, "fact").map_err(|e| ExecError::Failed(e.to_string()))?;
        let text = fact
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let tri = match fact.get("tri").and_then(Value::as_str) {
            Some("yes") => Some(Tri::Yes),
            Some("no") => Some(Tri::No),
            Some("unknown") => Some(Tri::Unknown),
            _ => None,
        };
        Ok(Observation { text, tri })
    }

    fn read_fact(&mut self, host: &Host, shape: &str) -> Result<Option<Vec<u8>>, ExecError> {
        let reply = self.call(execute_read_fact(host.name(), shape))?;
        Ok(reply
            .get("content")
            .and_then(Value::as_str)
            .map(|s| s.as_bytes().to_vec()))
    }

    fn bootstrap_state(&mut self, host: &Host) -> Result<BootstrapState, ExecError> {
        let reply = self.call(execute_op("bootstrap_state", host.name(), None))?;
        let s = field(&reply, "state").map_err(|e| ExecError::Failed(e.to_string()))?;
        serde_json::from_value(s.clone()).map_err(|e| ExecError::Failed(format!("R0303: {e}")))
    }

    /// `execute.clock`: a hook that does not serve it refuses, and the
    /// engine reads that as no skew probe rather than a broken hook.
    fn clock_now(&mut self, host: &Host) -> Result<Option<Instant>, ExecError> {
        let reply = match self.call(execute_op("clock", host.name(), None)) {
            Ok(r) => r,
            Err(ExecError::Failed(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        match reply.get("epoch_s").and_then(Value::as_u64) {
            Some(s) => Ok(Some(Instant::new(s))),
            None => Err(ExecError::Failed(
                "R0303: execute.clock without epoch_s".into(),
            )),
        }
    }

    fn instance_dir_create(&mut self, host: &Host, instance: &str) -> Result<(), ExecError> {
        self.call(execute_op(
            "instance_dir_create",
            host.name(),
            Some(instance),
        ))
        .map(|_| ())
    }

    fn instance_dir_remove(&mut self, host: &Host, instance: &str) -> Result<(), ExecError> {
        self.call(execute_op(
            "instance_dir_remove",
            host.name(),
            Some(instance),
        ))
        .map(|_| ())
    }

    fn instance_dir_list(&mut self, host: &Host) -> Result<Vec<InstanceDirState>, ExecError> {
        let reply = self.call(execute_op("instance_dir_list", host.name(), None))?;
        let dirs = field(&reply, "dirs").map_err(|e| ExecError::Failed(e.to_string()))?;
        serde_json::from_value(dirs.clone()).map_err(|e| ExecError::Failed(format!("R0303: {e}")))
    }

    fn put_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<(), ExecError> {
        let content = String::from_utf8_lossy(bytes);
        self.call(execute_put_file(host.name(), instance, rel, &content, mode))
            .map(|_| ())
    }

    fn replace_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
    ) -> Result<(), ExecError> {
        let content = String::from_utf8_lossy(bytes);
        self.call(execute_replace_file(host.name(), instance, rel, &content))
            .map(|_| ())
    }

    fn get_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<Vec<u8>, ExecError> {
        let reply = self.call(execute_get_file(host.name(), instance, rel))?;
        let c = field(&reply, "content").map_err(|e| ExecError::Failed(e.to_string()))?;
        Ok(c.as_str().unwrap_or("").as_bytes().to_vec())
    }

    fn remove_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<(), ExecError> {
        self.call(execute_remove_file(host.name(), instance, rel))
            .map(|_| ())
    }

    fn host_lock(&mut self, host: &Host) -> Result<Box<dyn HostLockGuard>, ExecError> {
        self.call(execute_op("host_lock", host.name(), None))?;
        Ok(Box::new(HookLock))
    }
}

struct HookLock;
impl HostLockGuard for HookLock {}

/// `journal to: hook(:name)`: the hook as a sink; a missing or silent hook
/// does not acknowledge (R0304 at the journal).
pub struct HookSink {
    pub name: String,
    pub registry: Arc<HookRegistry>,
    pub deadline: Duration,
}

impl Sink for HookSink {
    fn name(&self) -> String {
        format!("hook(:{})", self.name)
    }

    fn deliver(&mut self, e: &Entry) -> Result<(), String> {
        let (link, _) = self.registry.link(&self.name).map_err(|e| e.to_string())?;
        link.call(journal_append(e), self.deadline)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// `inventory from: hook(:name)`: the hosts the hook lists (Appendix C).
pub fn hook_inventory(
    registry: &HookRegistry,
    name: &str,
    deadline: Duration,
) -> Result<Vec<Host>, HookError> {
    let (link, _) = registry.link(name)?;
    let reply = link.call(inventory_list(), deadline)?;
    let hosts = field(&reply, "hosts")?;
    let records: Vec<InventoryHost> = serde_json::from_value(hosts.clone())
        .map_err(|e| HookError::Contract(format!("hosts: {e}")))?;
    Ok(records.into_iter().map(into_host).collect())
}

/// A host as a hook lists it (`rue_hook_proto::InventoryHost`, the
/// roadmap's Appendix C record) as the engine's own [`Host`]. The roles go
/// in as a fact so a clause dispatches on them exactly as it does for a
/// file inventory.
fn into_host(h: InventoryHost) -> Host {
    let mut facts = h.facts;
    if !h.roles.is_empty() {
        facts.insert("roles".into(), h.roles.join(","));
    }
    Host {
        record: HostRecord {
            name: h.name,
            os: h.os,
            reach: h.reach,
            filesystem: h.filesystem,
            stdin_preamble: h.stdin_preamble.unwrap_or(h.filesystem),
            artifact: h.artifact,
        },
        address: h.address,
        scheduler: h.scheduler,
        rue_root: h.rue_root,
        facts,
    }
}

/// `backstop scheduler: hook(:name)`: the hook as a scheduler binding
/// (7.5). The engine still owns the instance directory; this carries the
/// five ops of the `scheduler` kind.
pub struct HookScheduler {
    pub name: String,
    pub registry: Arc<HookRegistry>,
    pub deadline: Duration,
}

impl HookScheduler {
    fn call(&self, request: Value) -> Result<Value, ExecError> {
        let (link, _) = self
            .registry
            .link(&self.name)
            .map_err(|e| ExecError::Unreachable(e.to_string()))?;
        link.call(request, self.deadline).map_err(|e| match e {
            HookError::Silent => ExecError::Silent,
            HookError::Refused(r) => ExecError::Failed(r),
            HookError::Contract(m) => ExecError::Failed(format!("R0303: {m}")),
            HookError::Unregistered(n) => {
                ExecError::Unreachable(format!("hook {n} not registered"))
            }
            HookError::Io(m) => ExecError::Io(m),
        })
    }

    fn op(&self, op: &str, host: &Host, job: &Job, deadline: Option<u64>) -> Result<(), ExecError> {
        self.call(scheduler_op(op, host.name(), &job.artifact, deadline))
            .map(|_| ())
    }
}

impl Scheduler for HookScheduler {
    fn name(&self) -> &str {
        &self.name
    }

    fn install(&mut self, _ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError> {
        self.op("install", host, job, None)
    }

    fn arm(
        &mut self,
        _ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
        deadline: Instant,
    ) -> Result<(), ExecError> {
        self.op("arm", host, job, Some(deadline.unix_s))
    }

    fn rearm(
        &mut self,
        _ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
        deadline: Instant,
    ) -> Result<(), ExecError> {
        self.op("rearm", host, job, Some(deadline.unix_s))
    }

    fn disarm(&mut self, _ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError> {
        self.op("disarm", host, job, None)
    }

    fn present(
        &mut self,
        _ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
    ) -> Result<Presence, ExecError> {
        let reply = self.call(scheduler_op("present", host.name(), &job.artifact, None))?;
        Ok(match reply.get("present") {
            Some(Value::Bool(true)) => Presence::Present,
            Some(Value::Bool(false)) => Presence::Absent,
            Some(Value::String(s)) if s == "unknown" => Presence::Unknown,
            _ => {
                return Err(ExecError::Failed(
                    "R0303: scheduler.present without a present field".into(),
                ))
            }
        })
    }
}

/// `approval via: hook(:name)`: the hook as the approval binding (7.5).
/// It publishes the authenticators, renders the challenge and returns the
/// verdict; the digest and its scope are rue's, so a proof it accepts is
/// bound to one request and one scope.
pub struct HookApproval {
    pub name: String,
    pub registry: Arc<HookRegistry>,
    pub deadline: Duration,
}

impl HookApproval {
    fn call(&self, request: Value) -> Result<Value, ExecError> {
        let (link, _) = self
            .registry
            .link(&self.name)
            .map_err(|e| ExecError::Unreachable(e.to_string()))?;
        link.call(request, self.deadline).map_err(|e| match e {
            HookError::Silent => ExecError::Silent,
            HookError::Refused(r) => ExecError::Failed(r),
            HookError::Contract(m) => ExecError::Failed(format!("R0303: {m}")),
            HookError::Unregistered(n) => {
                ExecError::Unreachable(format!("hook {n} not registered"))
            }
            HookError::Io(m) => ExecError::Io(m),
        })
    }
}

fn scope_json(s: Scope) -> Value {
    match s {
        Scope::Plan => json!("plan"),
        Scope::Step(n) => json!({ "step": n }),
        Scope::Ack(n) => json!({ "ack": n }),
    }
}

impl Approval for HookApproval {
    fn name(&self) -> &str {
        &self.name
    }

    fn authenticators(&mut self) -> Result<Vec<Authenticator>, ExecError> {
        let reply = self.call(approval_authenticators())?;
        let v = field(&reply, "authenticators").map_err(|e| ExecError::Failed(e.to_string()))?;
        serde_json::from_value(v.clone()).map_err(|e| ExecError::Failed(format!("R0303: {e}")))
    }

    fn challenge(&mut self, r: &ProofRequest) -> Result<String, ExecError> {
        let reply = self.call(approval_challenge(
            &r.instance,
            &hex(&r.digest.0),
            scope_json(r.scope),
            json!(r.context),
        ))?;
        reply
            .get("challenge")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                ExecError::Failed("R0303: approval.challenge without a challenge".into())
            })
    }

    fn verify(&mut self, r: &ProofRequest) -> Result<Verified, ExecError> {
        let reply = self.call(approval_verify(
            &r.instance,
            &hex(&r.digest.0),
            scope_json(r.scope),
            &r.authenticator,
            &r.proof,
        ))?;
        match reply.get("verified").and_then(Value::as_bool) {
            Some(verified) => Ok(Verified {
                verified,
                reason: reply
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            }),
            None => Err(ExecError::Failed(
                "R0303: approval.verify without a verified field".into(),
            )),
        }
    }
}

/// `secrets deliver_to: hook(:name)`: the hook as a secret acceptor. This
/// is one of the four messages a secret may travel in (7.5); the reply is
/// an acceptance and a receipt, never the value again.
pub struct HookAcceptor {
    pub name: String,
    pub registry: Arc<HookRegistry>,
    pub deadline: Duration,
}

impl Acceptor for HookAcceptor {
    fn name(&self) -> &str {
        &self.name
    }

    fn deliver(
        &mut self,
        instance: &str,
        label: &str,
        value: &str,
        _now: Instant,
        _until: Option<Instant>,
    ) -> Result<bool, ExecError> {
        let (link, _) = self
            .registry
            .link(&self.name)
            .map_err(|e| ExecError::Unreachable(e.to_string()))?;
        let reply = link
            .call(secrets_deliver(instance, label, value), self.deadline)
            .map_err(|e| match e {
                HookError::Silent => ExecError::Silent,
                HookError::Refused(r) => ExecError::Failed(r),
                HookError::Contract(m) => ExecError::Failed(format!("R0303: {m}")),
                HookError::Unregistered(n) => {
                    ExecError::Unreachable(format!("hook {n} not registered"))
                }
                HookError::Io(m) => ExecError::Io(m),
            })?;
        match reply.get("accepted").and_then(Value::as_bool) {
            Some(a) => Ok(a),
            None => Err(ExecError::Failed(
                "R0303: secrets.deliver without an accepted field".into(),
            )),
        }
    }
}

/// `notify via: hook(:name)`: the hook as the notify binding (7.5).
pub struct HookNotify {
    pub name: String,
    pub registry: Arc<HookRegistry>,
    pub deadline: Duration,
}

impl Notify for HookNotify {
    fn name(&self) -> &str {
        &self.name
    }

    fn deliver(&mut self, level: Level, subject: &str, body: &str) -> Result<(), ExecError> {
        let (link, _) = self
            .registry
            .link(&self.name)
            .map_err(|e| ExecError::Unreachable(e.to_string()))?;
        link.call(notify_deliver(level.word(), subject, body), self.deadline)
            .map(|_| ())
            .map_err(|e| match e {
                HookError::Silent => ExecError::Silent,
                HookError::Refused(r) => ExecError::Failed(r),
                HookError::Contract(m) => ExecError::Failed(format!("R0303: {m}")),
                HookError::Unregistered(n) => {
                    ExecError::Unreachable(format!("hook {n} not registered"))
                }
                HookError::Io(m) => ExecError::Io(m),
            })
    }
}
