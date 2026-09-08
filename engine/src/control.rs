//! The control channel, docs/ROADMAP.md 7.4 and docs/control-protocol.md
//! v1: one socket, one handshake, then verbs; identity from peer
//! credentials, never from the client's word; hook registration on the
//! same connection after `hello`.
//!
//! Frames are newline-delimited JSON. A connection begins with
//! `{"hello": {"proto": N, "identity": ":name"}}`; the server maps the
//! peer's OS user to the declared operators and refuses anything else
//! (R0503). A `{"register": {...}}` frame after `hello` turns the
//! connection into a hook, accepted only from a declared registrar for a
//! name in its `may_register` (R0505). Every other frame is
//! `{"id", "verb", "args"}` answered by `{"id", "ok", "result" |
//! "error"}`. An act outside the operator's `operator_for` is R0504; an
//! admin verb by a non-admin is R0506; a protocol the server does not
//! speak is R0501.
//!
//! The server side is generic over the connection's reader and writer, so
//! a test drives it over a socket pair in one process with its own uid as
//! the peer; `serve` binds the real socket.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rue_core::ir::PlanIr;
use rue_core::journal::{Entry, Event as J};
use rue_core::ledger::LedgerCode;
use rue_core::model::{Duration as RDuration, ForceName, Mode};
use rue_core::states::RCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::hook::{HookRegistry, LineLink, Registered, Registration, HOOK_PROTOCOL};
use crate::journal::Sink;
use crate::lifecycle::{ApplyOptions, Engine, EngineError, InstanceRecord, Outcome};
use crate::peer::{peer_cred, user_name, PeerCred};

pub const CONTROL_PROTOCOL: u32 = 1;

// ---------------------------------------------------------------------------
// Operators (7.4)

/// The OS user an identity or a registrar maps to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserSpec {
    /// A named account.
    Name(String),
    /// The account the daemon runs as.
    SocketOwner,
}

impl UserSpec {
    /// From the site block's spelling: `:socket_owner` or a name.
    pub fn parse(s: &str) -> UserSpec {
        if s == "socket_owner" {
            UserSpec::SocketOwner
        } else {
            UserSpec::Name(s.to_string())
        }
    }

    fn matches(&self, peer: &Peer) -> bool {
        match self {
            UserSpec::SocketOwner => peer.cred.uid == peer.socket_owner_uid,
            UserSpec::Name(n) => peer.user.as_deref() == Some(n.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operator {
    pub name: String,
    pub user: UserSpec,
    /// Plan ids, or `all`.
    pub operator_for: Vec<String>,
    pub admin: bool,
    pub subscribe: Vec<String>,
}

impl Operator {
    pub fn admits_plan(&self, plan: &str) -> bool {
        self.operator_for.iter().any(|p| p == "all" || p == plan)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrarDecl {
    pub name: String,
    pub user: UserSpec,
    pub may_register: Vec<String>,
}

/// The declared operators and registrars of a site, and who the socket
/// owner is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operators {
    pub identities: Vec<Operator>,
    pub registrars: Vec<RegistrarDecl>,
    pub socket_owner_uid: u32,
    /// Daemon dry-run mode (7.9): with no operators block, every peer is
    /// the socket owner's `dry-run` identity.
    pub dry_run: bool,
}

/// The connecting process, as the kernel and the password database say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub cred: PeerCred,
    pub user: Option<String>,
    pub socket_owner_uid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlError {
    pub code: String,
    pub message: String,
}

impl ControlError {
    pub fn new(code: &str, message: impl Into<String>) -> ControlError {
        ControlError {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl Operators {
    /// The operator a peer is, given the identity its hello named (or none:
    /// the sole operator its user maps to). R0503 otherwise.
    pub fn identify(&self, peer: &Peer, requested: Option<&str>) -> Result<Operator, ControlError> {
        if self.identities.is_empty() && self.dry_run {
            return Ok(Operator {
                name: "dry-run".into(),
                user: UserSpec::SocketOwner,
                operator_for: vec!["all".into()],
                admin: true,
                subscribe: Vec::new(),
            });
        }
        let mine: Vec<&Operator> = self
            .identities
            .iter()
            .filter(|o| o.user.matches(peer))
            .collect();
        let who = peer
            .user
            .clone()
            .unwrap_or_else(|| format!("uid {}", peer.cred.uid));
        match requested {
            Some(name) => mine
                .iter()
                .find(|o| o.name == name)
                .map(|o| (*o).clone())
                .ok_or_else(|| {
                    ControlError::new(
                        "R0503",
                        format!("{who} is not declared as identity :{name}; group membership grants a connection, never an identity"),
                    )
                }),
            None => match mine.as_slice() {
                [one] => Ok((*one).clone()),
                [] => Err(ControlError::new(
                    "R0503",
                    format!("{who} maps to no declared operator; group membership grants a connection, never an identity"),
                )),
                many => Err(ControlError::new(
                    "R0503",
                    format!(
                        "{who} maps to {} identities ({}); the hello must name one",
                        many.len(),
                        many.iter().map(|o| format!(":{}", o.name)).collect::<Vec<_>>().join(", ")
                    ),
                )),
            },
        }
    }

    /// The registrar that may register `hook` from this peer, or R0505.
    pub fn registrar_for(&self, peer: &Peer, hook: &str) -> Result<RegistrarDecl, ControlError> {
        let who = peer
            .user
            .clone()
            .unwrap_or_else(|| format!("uid {}", peer.cred.uid));
        let mine: Vec<&RegistrarDecl> = self
            .registrars
            .iter()
            .filter(|r| r.user.matches(peer))
            .collect();
        if mine.is_empty() {
            return Err(ControlError::new(
                "R0505",
                format!("{who} is not a declared registrar"),
            ));
        }
        mine.iter()
            .find(|r| r.may_register.iter().any(|n| n == hook))
            .map(|r| (*r).clone())
            .ok_or_else(|| {
                ControlError::new(
                    "R0505",
                    format!(
                        "registrar {} may not register hook :{hook} (may_register: [{}])",
                        mine.iter()
                            .map(|r| format!(":{}", r.name))
                            .collect::<Vec<_>>()
                            .join(", "),
                        mine.iter()
                            .flat_map(|r| r.may_register.iter().map(|n| format!(":{n}")))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            })
    }
}

// ---------------------------------------------------------------------------
// Frames

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub proto: u32,
    #[serde(default)]
    pub identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloOk {
    pub ok: bool,
    pub proto: u32,
    pub identity: String,
    pub admin: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Request {
    pub id: u64,
    pub verb: String,
    #[serde(default)]
    pub args: Value,
}

/// The frames a client may send.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Hello(Hello),
    Register(Registration),
    Request(Request),
    /// A reply to a request the server sent (a registered hook answering).
    Reply(Value),
}

pub fn parse_frame(line: &str) -> Result<Frame, ControlError> {
    let v: Value = serde_json::from_str(line)
        .map_err(|e| ControlError::new("protocol", format!("not a JSON frame: {e}")))?;
    if let Some(h) = v.get("hello") {
        return serde_json::from_value(h.clone())
            .map(Frame::Hello)
            .map_err(|e| ControlError::new("protocol", format!("hello: {e}")));
    }
    if let Some(r) = v.get("register") {
        return serde_json::from_value(r.clone())
            .map(Frame::Register)
            .map_err(|e| ControlError::new("protocol", format!("register: {e}")));
    }
    if v.get("verb").is_some() {
        return serde_json::from_value(v)
            .map(Frame::Request)
            .map_err(|e| ControlError::new("protocol", format!("request: {e}")));
    }
    if v.get("ok").is_some() && v.get("id").is_some() {
        return Ok(Frame::Reply(v));
    }
    Err(ControlError::new(
        "protocol",
        "a frame is a hello, a register, a request (id, verb, args) or a reply (id, ok)",
    ))
}

fn write_line<W: Write>(w: &mut W, v: &Value) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(v)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

fn error_frame(id: Option<u64>, e: &ControlError) -> Value {
    let mut v = json!({ "ok": false, "error": { "code": e.code, "message": e.message } });
    if let Some(id) = id {
        v["id"] = json!(id);
    }
    v
}

// ---------------------------------------------------------------------------
// The daemon's shared state and the connection handler

/// Everything a connection handler needs.
pub struct Daemon {
    pub engine: Mutex<Engine>,
    pub operators: Operators,
    pub hooks: Arc<HookRegistry>,
    pub subscribers: Arc<Subscribers>,
    pub hook_deadline: Duration,
    pub dry_run: bool,
}

impl fmt::Debug for Daemon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Daemon(dry_run {}, hooks {:?})",
            self.dry_run,
            self.hooks.names()
        )
    }
}

/// Connections that receive `{"event": entry}` notifications for the plans
/// their operator subscribed to. A sink that never refuses: a
/// notification that cannot be written is dropped, never a journal
/// refusal.
#[derive(Default)]
pub struct Subscribers {
    list: Mutex<Vec<(Vec<String>, SharedWriter)>>,
}

impl Subscribers {
    pub fn add(&self, plans: Vec<String>, w: SharedWriter) {
        if !plans.is_empty() {
            self.list
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((plans, w));
        }
    }
}

/// The sink that fans entries out to subscribers.
pub struct SubscriberSink(pub Arc<Subscribers>);

impl Sink for SubscriberSink {
    fn name(&self) -> String {
        "subscribers".into()
    }
    fn deliver(&mut self, e: &Entry) -> Result<(), String> {
        let list = self.0.list.lock().unwrap_or_else(|e| e.into_inner());
        for (plans, w) in list.iter() {
            if plans.iter().any(|p| p == "all" || p == &e.plan) {
                if let Ok(mut w) = w.lock() {
                    let _ = write_line(&mut *w, &json!({ "event": e }));
                }
            }
        }
        Ok(())
    }
}

/// A shared writer for one connection.
pub type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Handle one connection to its end. Generic so a test can drive it over
/// a socket pair; `serve` calls it per accepted socket.
pub fn handle<R: BufRead>(mut reader: R, writer: SharedWriter, peer: Peer, daemon: &Daemon) {
    let mut line = String::new();
    // 1. hello
    let operator = loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let frame = match parse_frame(line.trim_end()) {
            Ok(f) => f,
            Err(e) => {
                let _ = write_line(
                    &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                    &error_frame(None, &e),
                );
                continue;
            }
        };
        match frame {
            Frame::Hello(h) => {
                if h.proto != CONTROL_PROTOCOL {
                    let e = ControlError::new(
                        "R0501",
                        format!("control protocol {} is not {CONTROL_PROTOCOL}", h.proto),
                    );
                    let _ = write_line(
                        &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                        &error_frame(None, &e),
                    );
                    return;
                }
                match daemon.operators.identify(&peer, h.identity.as_deref()) {
                    Ok(op) => {
                        let ok = HelloOk {
                            ok: true,
                            proto: CONTROL_PROTOCOL,
                            identity: op.name.clone(),
                            admin: op.admin,
                            dry_run: daemon.dry_run,
                        };
                        let _ = write_line(
                            &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                            &json!({ "hello": ok }),
                        );
                        break op;
                    }
                    Err(e) => {
                        let _ = write_line(
                            &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                            &error_frame(None, &e),
                        );
                        return;
                    }
                }
            }
            _ => {
                let e = ControlError::new("protocol", "the first frame is the hello");
                let _ = write_line(
                    &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                    &error_frame(None, &e),
                );
                return;
            }
        }
    };
    let _ = daemon.journal_site(J::OperatorConnected {
        identity: operator.name.clone(),
        admin: operator.admin,
    });
    daemon
        .subscribers
        .add(operator.subscribe.clone(), writer.clone());
    // 2. verbs, registration, replies
    let mut registered: Vec<(String, String)> = Vec::new();
    let mut link: Option<Arc<LineLink>> = None;
    let connection = format!("{}@uid{}", operator.name, peer.cred.uid);
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let frame = match parse_frame(line.trim_end()) {
            Ok(f) => f,
            Err(e) => {
                let _ = write_line(
                    &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                    &error_frame(None, &e),
                );
                continue;
            }
        };
        match frame {
            Frame::Hello(_) => {
                let e = ControlError::new("protocol", "hello was already said");
                let _ = write_line(
                    &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                    &error_frame(None, &e),
                );
            }
            Frame::Register(r) => {
                let reply = match register(
                    daemon,
                    &peer,
                    &operator,
                    &connection,
                    &r,
                    &writer,
                    &mut link,
                ) {
                    Ok(registrar) => {
                        registered.push((r.name.clone(), registrar));
                        json!({ "register": { "ok": true, "name": r.name } })
                    }
                    Err(e) => {
                        json!({ "register": { "ok": false, "error": { "code": e.code, "message": e.message } } })
                    }
                };
                let _ = write_line(
                    &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                    &reply,
                );
            }
            Frame::Request(req) => {
                let reply = match dispatch(daemon, &operator, &req.verb, &req.args) {
                    Ok(result) => json!({ "id": req.id, "ok": true, "result": result }),
                    Err(e) => error_frame(Some(req.id), &e),
                };
                let _ = write_line(
                    &mut *writer.lock().unwrap_or_else(|e| e.into_inner()),
                    &reply,
                );
            }
            Frame::Reply(v) => {
                if let Some(l) = &link {
                    l.deliver(v);
                }
            }
        }
    }
    for (name, registrar) in registered {
        daemon.hooks.deregister(&name);
        let _ = daemon.journal_site(J::HookDeregistered {
            name,
            registrar,
            reason: "connection closed".into(),
        });
    }
    let _ = daemon.journal_site(J::OperatorDisconnected {
        identity: operator.name.clone(),
    });
}

fn register(
    daemon: &Daemon,
    peer: &Peer,
    operator: &Operator,
    connection: &str,
    r: &Registration,
    writer: &SharedWriter,
    link: &mut Option<Arc<LineLink>>,
) -> Result<String, ControlError> {
    if r.protocol != HOOK_PROTOCOL {
        return Err(ControlError::new(
            "R0501",
            format!("hook protocol {} is not {HOOK_PROTOCOL}", r.protocol),
        ));
    }
    let registrar = daemon.operators.registrar_for(peer, &r.name)?;
    let l = match link {
        Some(l) => l.clone(),
        None => {
            let l = Arc::new(LineLink::new(
                &r.name,
                Box::new(SharedWrite(writer.clone())),
            ));
            *link = Some(l.clone());
            l
        }
    };
    daemon.hooks.register(Registered {
        registration: r.clone(),
        registrar: registrar.name.clone(),
        connection: connection.to_string(),
        link: l,
    });
    daemon
        .journal_site(J::HookRegistered {
            name: r.name.clone(),
            registrar: registrar.name.clone(),
            connection: format!("{connection} as :{}", operator.name),
        })
        .map_err(|e| ControlError::new("journal", e.to_string()))?;
    Ok(registrar.name)
}

/// A writer handle a link can own while the connection thread keeps its
/// own.
struct SharedWrite(SharedWriter);

impl Write for SharedWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).flush()
    }
}

impl Daemon {
    pub fn journal_site(&self, ev: J) -> Result<(), EngineError> {
        let mut e = self.engine.lock().unwrap_or_else(|e| e.into_inner());
        e.journal_site_event(ev).map(|_| ())
    }
}

// ---------------------------------------------------------------------------
// Verbs

fn engine_error(e: EngineError) -> ControlError {
    match e {
        EngineError::Refused(v) => ControlError {
            code: "refused".into(),
            message: rue_core::prose::prose(&v),
        },
        EngineError::Ledger(LedgerCode::R0101, m) => ControlError::new("R0101", m),
        EngineError::Ledger(LedgerCode::R0203, m) => ControlError::new("R0203", m),
        EngineError::NotAdmitted(RCode::R0102, m) => ControlError::new("R0102", m),
        EngineError::NotAdmitted(RCode::R0103, m) => ControlError::new("R0103", m),
        EngineError::NoSuchInstance(id) => ControlError::new("no_such_instance", id),
        EngineError::WrongState { state, verb } => ControlError::new(
            "wrong_state",
            format!("{verb} has no meaning in state {state}"),
        ),
        other => ControlError::new("engine", other.to_string()),
    }
}

fn outcome_json(o: &Outcome) -> Value {
    json!({ "id": o.id, "state": o.state.to_string(), "exit": o.exit, "line": o.line })
}

fn status_json(r: &InstanceRecord) -> Value {
    json!({
        "id": r.id,
        "plan": r.plan().id,
        "owner": r.plan().owner,
        "state": r.state.to_string(),
        "exit": crate::lifecycle::exit_of(r.state, false),
        "applied": r.applied.iter().map(|a| a.step).collect::<Vec<_>>(),
        "deadline": r.deadline.map(|d| d.unix_s),
        "waiting": r.waiting.as_ref().map(|w| json!({ "step": w.step, "reason": w.reason })),
        "held_at": r.held_at,
        "deferred": r.deferred.as_ref().map(|d| json!({ "step": d.step, "handoff": d.handoff })),
        "stuck": r.stuck,
        "drift_held": r.drift_held,
        "rehearsal": r.rehearsal,
        "closed_reason": r.closed_reason,
    })
}

fn arg_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, ControlError> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ControlError::new("protocol", format!("`{name}` (a string) is required")))
}

/// The scope check: the operator must admit the instance's plan (R0504).
fn scoped(daemon: &Daemon, op: &Operator, instance: &str) -> Result<String, ControlError> {
    let plan = {
        let e = daemon.engine.lock().unwrap_or_else(|e| e.into_inner());
        e.status(instance)
            .map_err(engine_error)?
            .map(|r| r.plan().id.clone())
            .ok_or_else(|| ControlError::new("no_such_instance", instance))?
    };
    if !op.admits_plan(&plan) {
        return Err(ControlError::new(
            "R0504",
            format!("identity :{} is not an operator for plan {plan}", op.name),
        ));
    }
    Ok(plan)
}

fn admin(op: &Operator, verb: &str) -> Result<(), ControlError> {
    if op.admin {
        Ok(())
    } else {
        Err(ControlError::new(
            "R0506",
            format!(
                "{verb} is an admin verb and :{} is not declared admin",
                op.name
            ),
        ))
    }
}

pub fn dispatch(
    daemon: &Daemon,
    op: &Operator,
    verb: &str,
    args: &Value,
) -> Result<Value, ControlError> {
    match verb {
        "apply" => {
            let ir: PlanIr = serde_json::from_value(
                args.get("ir")
                    .cloned()
                    .ok_or_else(|| ControlError::new("protocol", "`ir` is required"))?,
            )
            .map_err(|e| ControlError::new("protocol", format!("ir: {e}")))?;
            if !op.admits_plan(&ir.plan.id) {
                return Err(ControlError::new(
                    "R0504",
                    format!(
                        "identity :{} is not an operator for plan {}",
                        op.name, ir.plan.id
                    ),
                ));
            }
            let params: BTreeMap<String, String> = args
                .get("params")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| ControlError::new("protocol", format!("params: {e}")))?
                .unwrap_or_default();
            let acks: Vec<u32> = args
                .get("acks")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| ControlError::new("protocol", format!("acks: {e}")))?
                .unwrap_or_default();
            let forced: Vec<String> = args
                .get("forced")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| ControlError::new("protocol", format!("forced: {e}")))?
                .unwrap_or_default();
            let mode = match args.get("mode").and_then(Value::as_str) {
                Some("auto") => Some(Mode::Auto),
                Some("manual") => Some(Mode::Manual),
                Some(other) => return Err(ControlError::new("protocol", format!("mode {other}"))),
                None => None,
            };
            let rehearsal = daemon.dry_run
                || args
                    .get("rehearsal")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            let opts = ApplyOptions {
                rehearsal,
                acks,
                forced,
                mode,
                by: op.name.clone(),
            };
            let mut e = daemon.engine.lock().unwrap_or_else(|e| e.into_inner());
            let out = e.apply(ir, params, opts).map_err(engine_error)?;
            Ok(outcome_json(&out))
        }
        "status" => {
            let e = daemon.engine.lock().unwrap_or_else(|e| e.into_inner());
            match args.get("instance").and_then(Value::as_str) {
                Some(id) => {
                    let r = e
                        .status(id)
                        .map_err(engine_error)?
                        .ok_or_else(|| ControlError::new("no_such_instance", id))?;
                    if !op.admits_plan(&r.plan().id) {
                        return Err(ControlError::new(
                            "R0504",
                            format!(
                                "identity :{} is not an operator for plan {}",
                                op.name,
                                r.plan().id
                            ),
                        ));
                    }
                    Ok(status_json(&r))
                }
                None => Ok(Value::Array(
                    e.instances()
                        .map_err(engine_error)?
                        .iter()
                        .filter(|r| op.admits_plan(&r.plan().id))
                        .map(status_json)
                        .collect(),
                )),
            }
        }
        "recant" | "renew" | "confirm" | "commit" | "resume" | "handoff_done" | "cancel" => {
            let id = arg_str(args, "instance")?;
            scoped(daemon, op, id)?;
            let mut e = daemon.engine.lock().unwrap_or_else(|e| e.into_inner());
            let out = match verb {
                "recant" => {
                    let force: Vec<ForceName> = args
                        .get("force")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(|f| match f {
                                    "drift" => ForceName::Drift,
                                    "unknown" => ForceName::Unknown,
                                    g => ForceName::Guard(g.to_string()),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    e.recant(id, &force)
                }
                "renew" => {
                    let secs = args
                        .get("wane_s")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| ControlError::new("protocol", "`wane_s` is required"))?;
                    e.renew(id, RDuration::new(secs))
                }
                "confirm" => e.confirm(id),
                "commit" => {
                    let reason = arg_str(args, "reason")?;
                    e.commit(id, &op.name, reason)
                }
                "resume" => e.resume(id, &op.name),
                "handoff_done" => {
                    let step = args
                        .get("step")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| ControlError::new("protocol", "`step` is required"))?;
                    e.handoff_done(id, step as u32, &op.name)
                }
                "cancel" => e.cancel(id),
                _ => unreachable!(),
            }
            .map_err(engine_error)?;
            Ok(outcome_json(&out))
        }
        "abandon" => {
            admin(op, verb)?;
            let id = arg_str(args, "instance")?;
            let reason = arg_str(args, "reason")?;
            if reason.trim().is_empty() {
                return Err(ControlError::new("protocol", "abandon needs a reason"));
            }
            scoped(daemon, op, id)?;
            let mut e = daemon.engine.lock().unwrap_or_else(|e| e.into_inner());
            let out = e.abandon(id, &op.name, reason).map_err(engine_error)?;
            Ok(outcome_json(&out))
        }
        "hooks" => Ok(json!(daemon.hooks.names())),
        other => Err(ControlError::new(
            "protocol",
            format!("unknown verb {other}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// The socket

/// Bind the socket (removing a stale file), set its mode and group, and
/// accept connections until `stop` is set, each on its own thread.
pub fn serve(
    path: &Path,
    group: Option<u32>,
    daemon: Arc<Daemon>,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
    if let Some(gid) = group {
        let c = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
        // SAFETY: chown on a NUL-terminated path we own; -1 leaves the owner.
        let rc = unsafe { libc::chown(c.as_ptr(), u32::MAX, gid) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    listener.set_nonblocking(true)?;
    let socket_owner_uid = crate::peer::my_uid();
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let d = daemon.clone();
                std::thread::spawn(move || {
                    let _ = stream.set_nonblocking(false);
                    let cred = match peer_cred(&stream) {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    let peer = Peer {
                        cred,
                        user: user_name(cred.uid),
                        socket_owner_uid,
                    };
                    let reader = BufReader::new(match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => return,
                    });
                    let writer: SharedWriter = Arc::new(Mutex::new(Box::new(stream)));
                    handle(reader, writer, peer, &d);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The client

/// A client of the control channel: the CLI, a test, an embedding host.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

impl Client {
    pub fn connect(path: &Path) -> std::io::Result<Client> {
        let s = UnixStream::connect(path)?;
        Ok(Client {
            reader: BufReader::new(s.try_clone()?),
            writer: s,
            next_id: 1,
        })
    }

    fn read(&mut self) -> Result<Value, ControlError> {
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => {
                    return Err(ControlError::new(
                        "connection",
                        "the daemon closed the connection",
                    ))
                }
                Ok(_) => {}
                Err(e) => return Err(ControlError::new("connection", e.to_string())),
            }
            let v: Value = serde_json::from_str(line.trim_end())
                .map_err(|e| ControlError::new("protocol", e.to_string()))?;
            // Notifications interleave with replies; a client reading a
            // reply skips them.
            if v.get("event").is_some() {
                continue;
            }
            return Ok(v);
        }
    }

    pub fn hello(&mut self, identity: Option<&str>) -> Result<HelloOk, ControlError> {
        write_line(
            &mut self.writer,
            &json!({ "hello": Hello { proto: CONTROL_PROTOCOL, identity: identity.map(str::to_string) } }),
        )
        .map_err(|e| ControlError::new("connection", e.to_string()))?;
        let v = self.read()?;
        if let Some(h) = v.get("hello") {
            return serde_json::from_value(h.clone())
                .map_err(|e| ControlError::new("protocol", e.to_string()));
        }
        Err(error_of(&v))
    }

    pub fn register(&mut self, r: &Registration) -> Result<(), ControlError> {
        write_line(&mut self.writer, &json!({ "register": r }))
            .map_err(|e| ControlError::new("connection", e.to_string()))?;
        let v = self.read()?;
        match v.get("register") {
            Some(r) if r.get("ok") == Some(&Value::Bool(true)) => Ok(()),
            Some(r) => Err(error_of(r)),
            None => Err(error_of(&v)),
        }
    }

    pub fn call(&mut self, verb: &str, args: Value) -> Result<Value, ControlError> {
        let id = self.next_id;
        self.next_id += 1;
        write_line(
            &mut self.writer,
            &json!({ "id": id, "verb": verb, "args": args }),
        )
        .map_err(|e| ControlError::new("connection", e.to_string()))?;
        let v = self.read()?;
        if v.get("ok") == Some(&Value::Bool(true)) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(error_of(&v))
        }
    }

    /// The next frame the daemon sends this client (a request to a hook, or
    /// an event), as-is. A hook loop reads with this and answers with `send`.
    pub fn next_frame(&mut self) -> Result<Value, ControlError> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(ControlError::new(
                "connection",
                "the daemon closed the connection",
            )),
            Ok(_) => serde_json::from_str(line.trim_end())
                .map_err(|e| ControlError::new("protocol", e.to_string())),
            Err(e) => Err(ControlError::new("connection", e.to_string())),
        }
    }

    pub fn send(&mut self, v: &Value) -> Result<(), ControlError> {
        write_line(&mut self.writer, v).map_err(|e| ControlError::new("connection", e.to_string()))
    }
}

fn error_of(v: &Value) -> ControlError {
    match v.get("error") {
        Some(e) => ControlError::new(
            e.get("code").and_then(Value::as_str).unwrap_or("error"),
            e.get("message").and_then(Value::as_str).unwrap_or(""),
        ),
        None => ControlError::new("protocol", format!("unexpected frame: {v}")),
    }
}
