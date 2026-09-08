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
use rue_core::model::{HostRecord, Tri};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::executor::{
    BootstrapState, ExecCaps, ExecError, Executor, HostLockGuard, InstanceDirState, LocusKind,
    Observation, Output, ProbeRun, RPrim,
};
use crate::host::Host;
use crate::journal::Sink;

pub const HOOK_PROTOCOL: u32 = 1;

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

impl HookLink for LineLink {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn call(&self, mut request: Value, deadline: Duration) -> Result<Value, HookError> {
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
// Requests, one constructor per op of 7.5

pub fn req(kind: &str, op: &str) -> Value {
    json!({ "kind": kind, "op": op })
}

pub fn journal_append(e: &Entry) -> Value {
    let mut v = req("journal", "append");
    v["entry"] = serde_json::to_value(e).unwrap_or(Value::Null);
    v
}

pub fn inventory_list() -> Value {
    req("inventory", "list")
}

/// `execute.run`: one of the four messages that may carry a secret.
pub fn execute_run(host: &str, instance: &str, body: &[RPrim]) -> Value {
    let mut secrets = serde_json::Map::new();
    let mut plain = Vec::new();
    for (i, p) in body.iter().enumerate() {
        if p.carries_secret() {
            secrets.insert(format!("prim{i}"), json!(true));
        }
        plain.push(serde_json::to_value(p).unwrap_or(Value::Null));
    }
    let mut v = req("execute", "run");
    v["host"] = json!(host);
    v["instance"] = json!(instance);
    v["body"] = Value::Array(plain);
    v["env"] = json!({});
    v["secrets"] = Value::Object(secrets);
    v
}

pub fn execute_op(op: &str, host: &str, instance: Option<&str>) -> Value {
    let mut v = req("execute", op);
    v["host"] = json!(host);
    if let Some(i) = instance {
        v["instance"] = json!(i);
    }
    v
}

pub fn probe_observe(host: &str, probe: &str) -> Value {
    let mut v = req("probe", "observe");
    v["host"] = json!(host);
    v["probe"] = json!(probe);
    v
}

pub fn approval_authenticators() -> Value {
    req("approval", "authenticators")
}

pub fn approval_challenge(instance: &str, digest: &str, scope: Value, context: Value) -> Value {
    let mut v = req("approval", "challenge");
    v["instance"] = json!(instance);
    v["digest"] = json!(digest);
    v["scope"] = scope;
    v["context"] = context;
    v
}

pub fn approval_verify(
    instance: &str,
    digest: &str,
    scope: Value,
    authenticator: &str,
    proof: &str,
) -> Value {
    let mut v = req("approval", "verify");
    v["instance"] = json!(instance);
    v["digest"] = json!(digest);
    v["scope"] = scope;
    v["authenticator"] = json!(authenticator);
    v["proof"] = json!(proof);
    v
}

pub fn secrets_resolve(reference: &str) -> Value {
    let mut v = req("secrets", "resolve");
    v["ref"] = json!(reference);
    v
}

/// `secrets.deliver`: one of the four messages that may carry a secret.
pub fn secrets_deliver(instance: &str, label: &str, value: &str) -> Value {
    let mut v = req("secrets", "deliver");
    v["instance"] = json!(instance);
    v["label"] = json!(label);
    v["value"] = json!(value);
    v
}

pub fn notify_deliver(level: &str, subject: &str, body: &str) -> Value {
    let mut v = req("notify", "deliver");
    v["level"] = json!(level);
    v["subject"] = json!(subject);
    v["body"] = json!(body);
    v
}

pub fn scheduler_op(op: &str, host: &str, artifact: &str, deadline: Option<u64>) -> Value {
    let mut v = req("scheduler", op);
    v["host"] = json!(host);
    v["artifact"] = json!(artifact);
    v["deadline"] = json!(deadline);
    v
}

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
        let mut r = execute_op("read_fact", host.name(), None);
        r["shape"] = json!(shape);
        let reply = self.call(r)?;
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
        let mut r = execute_op("put_file", host.name(), Some(instance));
        r["rel"] = json!(rel);
        r["content"] = json!(String::from_utf8_lossy(bytes));
        r["mode"] = json!(mode);
        self.call(r).map(|_| ())
    }

    fn replace_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
    ) -> Result<(), ExecError> {
        let mut r = execute_op("replace_file", host.name(), Some(instance));
        r["rel"] = json!(rel);
        r["content"] = json!(String::from_utf8_lossy(bytes));
        self.call(r).map(|_| ())
    }

    fn get_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<Vec<u8>, ExecError> {
        let mut r = execute_op("get_file", host.name(), Some(instance));
        r["rel"] = json!(rel);
        let reply = self.call(r)?;
        let c = field(&reply, "content").map_err(|e| ExecError::Failed(e.to_string()))?;
        Ok(c.as_str().unwrap_or("").as_bytes().to_vec())
    }

    fn remove_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<(), ExecError> {
        let mut r = execute_op("remove_file", host.name(), Some(instance));
        r["rel"] = json!(rel);
        self.call(r).map(|_| ())
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
    Ok(records.into_iter().map(InventoryHost::into_host).collect())
}

/// A host as a hook lists it: the roadmap's Appendix C record.
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
    #[serde(default)]
    pub scheduler: Option<String>,
    #[serde(default)]
    pub facts: BTreeMap<String, String>,
}

impl InventoryHost {
    pub fn into_host(self) -> Host {
        let mut facts = self.facts;
        if !self.roles.is_empty() {
            facts.insert("roles".into(), self.roles.join(","));
        }
        Host {
            record: HostRecord {
                name: self.name,
                os: self.os,
                reach: self.reach,
                filesystem: self.filesystem,
                stdin_preamble: self.filesystem,
                artifact: None,
            },
            address: self.address,
            scheduler: self.scheduler,
            rue_root: None,
            facts,
        }
    }
}
