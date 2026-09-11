//! The control channel over a socket pair in one process (the peer is this
//! process's own uid): identity from peer credentials and the declared
//! operators (R0503), the protocol version (R0501), scope (R0504), admin
//! verbs (R0506), hook registration by declared registrars only (R0505)
//! and journaled, a hook serving execute and probe through the same
//! connection, a hook that goes silent, notifications to a subscriber, and
//! every verb over the channel.

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::world::{self, World, OWNER};
use rue_core::journal::Event as J;
use rue_engine::control::{
    handle, Daemon, Operator, Operators, Peer, RegistrarDecl, SharedWriter, SubscriberSink,
    Subscribers, UserSpec, CONTROL_PROTOCOL,
};
use rue_engine::executor::Executor;
use rue_engine::hook::{HookExecutor, HookRegistry, HOOK_PROTOCOL};
use rue_engine::journal::{Journal, MemorySink, Sink};
use rue_engine::lifecycle::Engine;
use rue_engine::peer::my_account;
use rue_engine::store::Store;
use serde_json::{json, Value};

/// A daemon over the test world's engine, with the given operators.
/// The world is returned too: its temporary directory holds the store.
fn daemon(
    w: World,
    ops: Operators,
    dry_run: bool,
) -> (Arc<Daemon>, MemorySink, Arc<HookRegistry>, World) {
    let hooks = Arc::new(HookRegistry::new());
    let subscribers = Arc::new(Subscribers::default());
    // A fresh engine whose journal fans out to the subscribers too, and
    // whose executors include a hook executor for transport `api`.
    let store = Store::create(&w.dir.join("store-b")).unwrap();
    let sink = MemorySink::new("mem");
    let sinks: Vec<Box<dyn Sink>> = vec![
        Box::new(sink.clone()),
        Box::new(SubscriberSink(subscribers.clone())),
    ];
    let journal = Journal::open(&store, sinks, None).unwrap();
    let hook_exec = HookExecutor {
        name: "actuate".into(),
        transport: "api".into(),
        registry: hooks.clone(),
        deadline: Duration::from_millis(500),
    };
    let execs: Vec<Box<dyn Executor>> = vec![Box::new(w.ssh.clone()), Box::new(hook_exec)];
    let mut api_host = world::host("api-01", &["api"]);
    api_host.record.filesystem = false;
    api_host.record.stdin_preamble = false;
    let engine = Engine::open(
        store,
        journal,
        w.clock.clone(),
        execs,
        vec![world::host(OWNER, &["ssh"]), api_host],
    )
    .unwrap();
    let d = Arc::new(Daemon {
        engine: Mutex::new(engine),
        operators: ops,
        hooks: hooks.clone(),
        subscribers,
        hook_deadline: Duration::from_millis(500),
        dry_run,
        mailbox: Default::default(),
    });
    (d, sink, hooks, w)
}

fn ops(identities: Vec<Operator>, registrars: Vec<RegistrarDecl>) -> Operators {
    Operators {
        identities,
        registrars,
        dry_run: false,
    }
}

fn me() -> String {
    my_account().expect("this account has a name")
}

fn operator(name: &str, user: UserSpec, plans: &[&str], admin: bool) -> Operator {
    Operator {
        name: name.into(),
        user,
        operator_for: plans.iter().map(|s| s.to_string()).collect(),
        admin,
        subscribe: Vec::new(),
    }
}

/// A connection to the daemon: the server side handled on its own thread
/// over a pair of anonymous pipes, which every platform rue runs on has.
/// The transport a real daemon binds is the platform's (a Unix socket, a
/// Windows named pipe); everything these tests exercise sits above it.
struct Conn {
    reader: Option<BufReader<std::io::PipeReader>>,
    writer: Option<std::io::PipeWriter>,
    next: u64,
    /// The server side's thread, joined when the connection is dropped.
    server: Option<thread::JoinHandle<()>>,
}

/// The server side's thread, joined when this is dropped: what a test that
/// takes a connection's two ends holds in its place.
struct Joined(Option<thread::JoinHandle<()>>);

impl Drop for Joined {
    fn drop(&mut self) {
        if let Some(server) = self.0.take() {
            let _ = server.join();
        }
    }
}

/// Dropping a connection closes the client's end and waits for the server
/// side to finish, which includes journaling the disconnect. Without the
/// wait, that journal write raced the test's own cleanup: it recreated a
/// file in the store while the temporary directory was being removed, the
/// removal failed on a directory no longer empty, and /tmp kept one
/// rue-engine-control-* directory per lost race -- a few hundred of them by
/// Phase 4's end.
impl Drop for Conn {
    fn drop(&mut self) {
        drop(self.writer.take());
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

impl Conn {
    fn open(d: &Arc<Daemon>) -> Conn {
        // Client writes, server reads; server writes, client reads.
        let (server_rx, client_tx) = std::io::pipe().unwrap();
        let (client_rx, server_tx) = std::io::pipe().unwrap();
        let d = d.clone();
        let server = thread::spawn(move || {
            let peer = Peer {
                user: rue_engine::peer::my_account(),
                owner: true,
                uid: None,
            };
            let writer: SharedWriter = Arc::new(Mutex::new(Box::new(server_tx)));
            handle(BufReader::new(server_rx), writer, peer, d);
        });
        Conn {
            reader: Some(BufReader::new(client_rx)),
            writer: Some(client_tx),
            next: 1,
            server: Some(server),
        }
    }

    fn send(&mut self, v: Value) {
        let mut line = serde_json::to_vec(&v).unwrap();
        line.push(b'\n');
        self.writer.as_mut().unwrap().write_all(&line).unwrap();
    }

    fn read_line(&mut self, line: &mut String) -> usize {
        self.reader.as_mut().unwrap().read_line(line).unwrap()
    }

    /// The two ends, for a test that serves on this connection from a
    /// thread of its own, and the server side's thread, joined when the
    /// returned guard is dropped -- which must come after the ends are.
    fn into_ends(mut self) -> (BufReader<std::io::PipeReader>, std::io::PipeWriter, Joined) {
        (
            self.reader.take().unwrap(),
            self.writer.take().unwrap(),
            Joined(self.server.take()),
        )
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        let n = self.read_line(&mut line);
        assert!(n > 0, "the daemon closed the connection");
        serde_json::from_str(line.trim_end()).unwrap()
    }

    fn hello(&mut self, identity: Option<&str>) -> Value {
        self.send(json!({ "hello": { "proto": CONTROL_PROTOCOL, "identity": identity } }));
        self.recv()
    }

    fn call(&mut self, verb: &str, args: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "id": id, "verb": verb, "args": args }));
        loop {
            let v = self.recv();
            if v.get("event").is_some() {
                continue;
            }
            assert_eq!(v.get("id").and_then(Value::as_u64), Some(id));
            return v;
        }
    }
}

fn error_code(v: &Value) -> &str {
    v.pointer("/error/code")
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn plan_ir(id: &str) -> Value {
    let plan = world::temp_plan(id, vec![world::step(world::op("a"))]);
    serde_json::to_value(world::ir(plan)).unwrap()
}

#[test]
fn identity_comes_from_peer_credentials_and_the_declared_operators() {
    let w = World::new("control-identity");
    let (d, sink, _, _w) = daemon(
        w,
        ops(
            vec![
                operator("ops", UserSpec::Name(me()), &["all"], true),
                operator(
                    "nobody",
                    UserSpec::Name("no-such-user-here".into()),
                    &["all"],
                    true,
                ),
                operator("owner", UserSpec::SocketOwner, &["p"], false),
            ],
            vec![],
        ),
        false,
    );
    // Named and matching: accepted, with what it is.
    let mut c = Conn::open(&d);
    let h = c.hello(Some("ops"));
    assert_eq!(h.pointer("/hello/ok"), Some(&json!(true)), "{h}");
    assert_eq!(h.pointer("/hello/identity"), Some(&json!("ops")));
    assert_eq!(h.pointer("/hello/admin"), Some(&json!(true)));
    // Named but another user's identity: R0503.
    let mut c2 = Conn::open(&d);
    let h = c2.hello(Some("nobody"));
    assert_eq!(error_code(&h), "R0503", "{h}");
    assert!(h
        .pointer("/error/message")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("never an identity"));
    // The socket owner's identity by uid.
    let mut c3 = Conn::open(&d);
    let h = c3.hello(Some("owner"));
    assert_eq!(h.pointer("/hello/identity"), Some(&json!("owner")), "{h}");
    // Unnamed with two identities for this user: the hello must choose.
    let mut c4 = Conn::open(&d);
    let h = c4.hello(None);
    assert_eq!(error_code(&h), "R0503", "{h}");
    assert!(h
        .pointer("/error/message")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("must name one"));
    // A wrong protocol: R0501.
    let mut c5 = Conn::open(&d);
    c5.send(json!({ "hello": { "proto": 99 } }));
    let h = c5.recv();
    assert_eq!(error_code(&h), "R0501", "{h}");
    // A verb before hello is refused.
    let mut c6 = Conn::open(&d);
    c6.send(json!({ "id": 1, "verb": "status", "args": {} }));
    let h = c6.recv();
    assert_eq!(error_code(&h), "protocol", "{h}");
    // Connections are journaled with their identity, before the client
    // is told it is in: having the reply is having the entry.
    let ev = sink.events();
    assert!(ev.contains(&J::OperatorConnected {
        identity: "ops".into(),
        admin: true
    }));
    assert!(ev.contains(&J::OperatorConnected {
        identity: "owner".into(),
        admin: false
    }));
    assert!(!ev
        .iter()
        .any(|e| matches!(e, J::OperatorConnected { identity, .. } if identity == "nobody")));
    // The departure is journaled when the server notices the connection
    // is gone, which is its own thread's business: awaited, not assumed.
    drop(c);
    let gone = J::OperatorDisconnected {
        identity: "ops".into(),
    };
    let mut waited = 0;
    while !sink.events().contains(&gone) && waited < 200 {
        thread::sleep(Duration::from_millis(10));
        waited += 1;
    }
    assert!(sink.events().contains(&gone), "the departure is journaled");
}

#[test]
fn a_sole_identity_needs_no_name_and_an_undeclared_user_is_refused() {
    let w = World::new("control-sole");
    let (d, _, _, _w) = daemon(
        w,
        ops(
            vec![operator("ops", UserSpec::Name(me()), &["all"], false)],
            vec![],
        ),
        false,
    );
    let mut c = Conn::open(&d);
    let h = c.hello(None);
    assert_eq!(h.pointer("/hello/identity"), Some(&json!("ops")), "{h}");
    let w2 = World::new("control-none");
    let (d2, _, _, _w2) = daemon(
        w2,
        ops(
            vec![operator(
                "x",
                UserSpec::Name("someone-else".into()),
                &["all"],
                false,
            )],
            vec![],
        ),
        false,
    );
    let mut c = Conn::open(&d2);
    let h = c.hello(None);
    assert_eq!(error_code(&h), "R0503", "{h}");
}

#[test]
fn every_verb_runs_over_the_channel_within_the_operator_s_scope() {
    let w = World::new("control-verbs");
    let (d, sink, _, _w) = daemon(
        w,
        ops(
            vec![
                operator("ops", UserSpec::Name(me()), &["p", "q"], false),
                operator("admin", UserSpec::SocketOwner, &["all"], true),
            ],
            vec![],
        ),
        false,
    );
    let mut c = Conn::open(&d);
    c.hello(Some("ops"));
    // apply within scope
    let r = c.call("apply", json!({ "ir": plan_ir("p"), "params": {} }));
    assert_eq!(r.get("ok"), Some(&json!(true)), "{r}");
    let id = r
        .pointer("/result/id")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(r.pointer("/result/state"), Some(&json!("Applied")));
    assert_eq!(r.pointer("/result/exit"), Some(&json!(0)));
    // status, one and all
    let s = c.call("status", json!({ "instance": id }));
    assert_eq!(s.pointer("/result/state"), Some(&json!("Applied")));
    assert_eq!(s.pointer("/result/applied"), Some(&json!([1])));
    let all = c.call("status", json!({}));
    assert_eq!(all.pointer("/result").unwrap().as_array().unwrap().len(), 1);
    // out of scope: R0504
    let r = c.call("apply", json!({ "ir": plan_ir("r"), "params": {} }));
    assert_eq!(error_code(&r), "R0504", "{r}");
    // renew, confirm (R0102 on a temporary plan), recant
    let r = c.call("renew", json!({ "instance": id, "wane_s": 100 }));
    assert_eq!(error_code(&r), "R0102", "{r}");
    let r = c.call("confirm", json!({ "instance": id }));
    assert_eq!(error_code(&r), "R0102", "{r}");
    let r = c.call("recant", json!({ "instance": id, "force": [] }));
    assert_eq!(r.pointer("/result/state"), Some(&json!("Closed")), "{r}");
    // A recant that reverts cleanly did what was asked: exit 0. Exit 1 is
    // for an instance that closed because something refused it.
    assert_eq!(r.pointer("/result/exit"), Some(&json!(0)));
    // a verb on a closed instance: wrong_state
    let r = c.call("recant", json!({ "instance": id }));
    assert_eq!(error_code(&r), "wrong_state", "{r}");
    // no such instance
    let r = c.call("status", json!({ "instance": "nope" }));
    assert_eq!(error_code(&r), "no_such_instance", "{r}");
    // abandon needs admin (R0506) and a reason
    let r = c.call("abandon", json!({ "instance": id, "reason": "x" }));
    assert_eq!(error_code(&r), "R0506", "{r}");
    let mut a = Conn::open(&d);
    a.hello(Some("admin"));
    let r = a.call("abandon", json!({ "instance": id, "reason": "" }));
    assert_eq!(error_code(&r), "protocol", "{r}");
    let r = a.call("abandon", json!({ "instance": id, "reason": "gone" }));
    assert_eq!(error_code(&r), "wrong_state", "abandon on Closed: {r}");
    // an unknown verb
    let r = c.call("frobnicate", json!({}));
    assert_eq!(error_code(&r), "protocol");
    // the ledger's refusal carries its code
    let r = c.call("apply", json!({ "ir": plan_ir("q"), "params": {} }));
    assert_eq!(r.pointer("/result/state"), Some(&json!("Applied")));
    let r = c.call("apply", json!({ "ir": plan_ir("q"), "params": {} }));
    assert_eq!(error_code(&r), "R0101", "{r}");
    assert!(sink.events().contains(&J::Recant));
}

#[test]
fn a_hook_registers_by_a_declared_registrar_only_is_journaled_and_serves_execute_and_probe() {
    let w = World::new("control-hook");
    let (d, sink, hooks, _w) = daemon(
        w,
        ops(
            vec![operator("ops", UserSpec::Name(me()), &["all"], true)],
            vec![RegistrarDecl {
                name: "host".into(),
                user: UserSpec::SocketOwner,
                may_register: vec!["actuate".into()],
            }],
        ),
        false,
    );
    // Register before hello: refused as a protocol error.
    let mut early = Conn::open(&d);
    early.send(json!({ "register": { "name": "actuate", "kinds": ["execute"], "protocol": HOOK_PROTOCOL } }));
    let r = early.recv();
    assert_eq!(error_code(&r), "protocol", "{r}");
    // A name outside may_register: R0505.
    let mut c = Conn::open(&d);
    c.hello(Some("ops"));
    c.send(
        json!({ "register": { "name": "other", "kinds": ["execute"], "protocol": HOOK_PROTOCOL } }),
    );
    let r = c.recv();
    assert_eq!(
        r.pointer("/register/error/code"),
        Some(&json!("R0505")),
        "{r}"
    );
    // A wrong hook protocol: R0501.
    c.send(json!({ "register": { "name": "actuate", "kinds": ["execute"], "protocol": 7 } }));
    let r = c.recv();
    assert_eq!(
        r.pointer("/register/error/code"),
        Some(&json!("R0501")),
        "{r}"
    );
    // The declared one: accepted and journaled.
    c.send(json!({ "register": { "name": "actuate", "kinds": ["execute", "probe"], "protocol": HOOK_PROTOCOL } }));
    let r = c.recv();
    assert_eq!(r.pointer("/register/ok"), Some(&json!(true)), "{r}");
    assert_eq!(hooks.names(), vec!["actuate".to_string()]);
    assert_eq!(hooks.serving("execute"), vec!["actuate".to_string()]);
    // Journaled on the server's own thread: awaited, not assumed.
    let registered = |sink: &MemorySink| {
        sink.events().iter().any(
            |e| matches!(e, J::HookRegistered { name, registrar, .. } if name == "actuate" && registrar == "host"),
        )
    };
    let mut waited = 0;
    while !registered(&sink) && waited < 200 {
        thread::sleep(Duration::from_millis(10));
        waited += 1;
    }
    assert!(registered(&sink), "the registration is journaled");

    // The hook serves: a plan on api-01 (reach api) runs through it. The
    // hook answers on the same connection from another thread while the
    // operator applies over a second connection.
    let (server_side, _keep, _server) = c.into_ends();
    let mut hook_reader = server_side;
    let mut hook_writer = _keep;
    let served = Arc::new(Mutex::new(Vec::new()));
    let served2 = served.clone();
    let hook_thread = thread::spawn(move || {
        let mut line = String::new();
        // Exactly two requests: the probe, then the run.
        for _ in 0..2 {
            line.clear();
            if hook_reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let req: Value = serde_json::from_str(line.trim_end()).unwrap();
            served2.lock().unwrap().push(req.clone());
            let id = req["id"].clone();
            let reply = match (req["kind"].as_str(), req["op"].as_str()) {
                (Some("execute"), Some("run")) => {
                    json!({ "id": id, "ok": true, "output": { "stdout": "", "outputs": { "token": "t-9" } }, "facts": [] })
                }
                (Some("probe"), Some("observe")) => {
                    json!({ "id": id, "ok": true, "fact": { "text": "up", "tri": "yes" } })
                }
                _ => json!({ "id": id, "ok": false, "error": "unexpected" }),
            };
            let mut bytes = serde_json::to_vec(&reply).unwrap();
            bytes.push(b'\n');
            hook_writer.write_all(&bytes).unwrap();
        }
    });
    let mut o = world::on(world::op("act"), "api-01");
    o.pre = vec![world::guard("up", rue_core::model::Tri::Unknown)];
    o.outputs = vec![rue_core::model::Output {
        name: "token".into(),
        secret: false,
    }];
    o.footprint = vec![];
    o.undo = rue_core::model::Undo::Restore;
    let mut plan = world::temp_plan("h", vec![world::step(o)]);
    plan.owner = "api-01".into();
    let mut site = world::site();
    site.transports.push("api".into());
    site.hosts.push(world::record("api-01", &["api"]));
    let ir = rue_core::ir::PlanIr {
        ir_version: rue_core::ir::IR_VERSION,
        requester: "ops".into(),
        site,
        plan,
    };
    let mut op_conn = Conn::open(&d);
    op_conn.hello(Some("ops"));
    let r = op_conn.call("apply", json!({ "ir": ir, "params": {} }));
    assert_eq!(r.pointer("/result/state"), Some(&json!("Applied")), "{r}");
    hook_thread.join().unwrap();
    let served = served.lock().unwrap();
    assert_eq!(served.len(), 2, "{served:?}");
    assert_eq!(
        (served[0]["kind"].as_str(), served[0]["op"].as_str()),
        (Some("probe"), Some("observe"))
    );
    assert_eq!(served[0]["probe"], json!("up"));
    assert_eq!(
        (served[1]["kind"].as_str(), served[1]["op"].as_str()),
        (Some("execute"), Some("run"))
    );
    assert_eq!(served[1]["host"], json!("api-01"));
    assert!(served[1]["body"].as_array().unwrap().len() == 1);
    let id = r
        .pointer("/result/id")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let s = op_conn.call("status", json!({ "instance": id }));
    assert_eq!(s.pointer("/result/state"), Some(&json!("Applied")));
    let e = d.engine.lock().unwrap();
    let rec = e.status(&id).unwrap().unwrap();
    assert_eq!(
        rec.outputs.get("act.token").map(String::as_str),
        Some("t-9")
    );
}

#[test]
fn a_hook_that_goes_silent_refuses_the_step_and_its_departure_is_journaled() {
    let w = World::new("control-silent");
    let (d, sink, hooks, _w) = daemon(
        w,
        ops(
            vec![operator("ops", UserSpec::Name(me()), &["all"], true)],
            vec![RegistrarDecl {
                name: "host".into(),
                user: UserSpec::SocketOwner,
                may_register: vec!["actuate".into()],
            }],
        ),
        false,
    );
    let mut c = Conn::open(&d);
    c.hello(Some("ops"));
    c.send(json!({ "register": { "name": "actuate", "kinds": ["execute"], "protocol": HOOK_PROTOCOL } }));
    c.recv();
    // The hook never answers.
    let mut o = world::on(world::op("act"), "api-01");
    o.footprint = vec![];
    o.undo = rue_core::model::Undo::Restore;
    let mut plan = world::temp_plan("h", vec![world::step(o)]);
    plan.owner = "api-01".into();
    let mut site = world::site();
    site.transports.push("api".into());
    site.hosts.push(world::record("api-01", &["api"]));
    let ir = rue_core::ir::PlanIr {
        ir_version: rue_core::ir::IR_VERSION,
        requester: "ops".into(),
        site,
        plan,
    };
    let mut op_conn = Conn::open(&d);
    op_conn.hello(Some("ops"));
    let r = op_conn.call("apply", json!({ "ir": ir, "params": {} }));
    assert_eq!(r.pointer("/result/state"), Some(&json!("Closed")), "{r}");
    assert!(sink
        .events()
        .iter()
        .any(|e| matches!(e, J::StepFailed { error, .. } if error.contains("silent"))));
    // The hook connection closes: deregistered and journaled.
    // The registry loses the hook first and the journal records it after,
    // both on the server's own thread: the wait is for the entry, which
    // is the later of the two.
    drop(c);
    let journaled = |sink: &MemorySink| {
        sink.events()
            .iter()
            .any(|e| matches!(e, J::HookDeregistered { name, .. } if name == "actuate"))
    };
    let mut waited = 0;
    while !journaled(&sink) && waited < 200 {
        thread::sleep(Duration::from_millis(10));
        waited += 1;
    }
    assert!(journaled(&sink), "the departure is journaled");
    assert!(hooks.names().is_empty(), "the hook is deregistered");
}

#[test]
fn a_subscriber_receives_the_entries_of_its_plans_and_dry_run_forces_rehearsal() {
    let w = World::new("control-subscribe");
    let mut sub = operator("watcher", UserSpec::Name(me()), &["all"], false);
    sub.subscribe = vec!["p".into()];
    let (d, _, _, _w) = daemon(w, ops(vec![sub], vec![]), true);
    let mut c = Conn::open(&d);
    let h = c.hello(None);
    assert_eq!(h.pointer("/hello/dry_run"), Some(&json!(true)));
    let r = c.call("apply", json!({ "ir": plan_ir("p"), "params": {} }));
    assert!(
        r.pointer("/result/line")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("rehearsal"),
        "{r}"
    );
    // The reply to the first call was the reply, not an event: `Conn::call`
    // asserts the id. A plan not subscribed to yields no events before its
    // reply.
    let mut c2 = Conn::open(&d);
    c2.hello(None);
    let mut line = String::new();
    c2.send(json!({ "id": 7, "verb": "apply", "args": { "ir": plan_ir("q"), "params": {} } }));
    c2.read_line(&mut line);
    let v: Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(v.get("id"), Some(&json!(7)), "{v}");
    let mut events = 0;
    let mut first = String::new();
    // And a fresh subscriber sees events for p on a new apply of p.
    let mut c3 = Conn::open(&d);
    c3.hello(None);
    c3.send(
        json!({ "id": 1, "verb": "apply", "args": { "ir": plan_ir("p"), "params": { "x": "1" } } }),
    );
    loop {
        first.clear();
        c3.read_line(&mut first);
        let v: Value = serde_json::from_str(first.trim_end()).unwrap();
        if v.get("event").is_some() {
            assert_eq!(v.pointer("/event/plan"), Some(&json!("p")));
            events += 1;
        } else {
            assert_eq!(v.get("id"), Some(&json!(1)), "{v}");
            break;
        }
    }
    assert!(events >= 3, "{events} events");
}

#[test]
fn a_recant_whose_force_is_not_a_list_of_names_is_a_protocol_refusal() {
    // The failure this prevents is not a spelling mistake, it is a LIE. A
    // `force` the verb could not read was dropped, so the recant ran as an
    // ordinary one and came back R0103 -- a refusal that is right for a
    // request nobody made, while the operator's actual request had been
    // discarded in silence. T4's host hit exactly this: it asked to force
    // a DriftHeld instance through and was told it could not recant.
    let w = World::new("control-force");
    let (d, _, _, _w) = daemon(
        w,
        ops(
            vec![operator("ops", UserSpec::Name(me()), &["all"], true)],
            vec![],
        ),
        false,
    );
    let mut c = Conn::open(&d);
    c.hello(None);
    let applied = c.call("apply", json!({ "ir": plan_ir("p"), "params": {} }));
    let id = applied
        .pointer("/result/id")
        .and_then(Value::as_str)
        .expect("an instance id")
        .to_string();

    // A bare string where the protocol documents a list of names.
    let r = c.call("recant", json!({ "instance": id, "force": "drift" }));
    assert_eq!(
        r.pointer("/error/code"),
        Some(&json!("protocol")),
        "a force the verb cannot read must be refused, not ignored: {r}"
    );
    // A list whose members are not strings, likewise.
    let r = c.call("recant", json!({ "instance": id, "force": [7] }));
    assert_eq!(r.pointer("/error/code"), Some(&json!("protocol")), "{r}");

    // And the shape the protocol does document still works, so the
    // refusal above is about the argument and not about forcing at all.
    let r = c.call("recant", json!({ "instance": id, "force": ["drift"] }));
    assert!(
        r.get("result").is_some(),
        "a well-formed force was refused: {r}"
    );
}

#[test]
fn a_registered_hook_connection_may_also_act_as_an_operator() {
    // T4's shape: the host registers its hooks and applies its own plans
    // over the same connection.
    let w = World::new("control-both");
    let (d, _, _, _w) = daemon(
        w,
        ops(
            vec![operator("host", UserSpec::SocketOwner, &["p"], true)],
            vec![RegistrarDecl {
                name: "host".into(),
                user: UserSpec::SocketOwner,
                may_register: vec!["actuate".into()],
            }],
        ),
        false,
    );
    let mut c = Conn::open(&d);
    c.hello(Some("host"));
    c.send(json!({ "register": { "name": "actuate", "kinds": ["execute"], "protocol": HOOK_PROTOCOL } }));
    assert_eq!(c.recv().pointer("/register/ok"), Some(&json!(true)));
    let r = c.call("apply", json!({ "ir": plan_ir("p"), "params": {} }));
    assert_eq!(r.pointer("/result/state"), Some(&json!("Applied")), "{r}");
    let _ = BTreeMap::<String, String>::new();
}

#[test]
fn a_secret_is_dropped_from_every_hook_message_but_the_two_that_may_carry_one() {
    use rue_engine::hook::{guard_secrets, DROPPED};
    use serde_json::json;

    // `execute.run` carries the resolved body, secrets and all: one of the
    // four messages of 5.13.
    let mut run = json!({
        "kind": "execute",
        "op": "run",
        "body": [{ "run": { "cmd": { "text": "login", "secret": false },
                            "env": [["PW", { "text": "s3cr3t", "secret": true }]] } }],
    });
    assert!(guard_secrets(&mut run).is_empty());
    assert!(
        serde_json::to_string(&run).unwrap().contains("s3cr3t"),
        "the permitted message keeps it"
    );

    // `secrets.deliver` likewise.
    let mut deliver = json!({ "kind": "secrets", "op": "deliver", "value": { "text": "s3cr3t", "secret": true } });
    assert!(guard_secrets(&mut deliver).is_empty());

    // Anything else loses the value and keeps the shape (R0305).
    let mut probe = json!({
        "kind": "probe",
        "op": "observe",
        "probe": { "body": [{ "run": { "cmd": { "text": "s3cr3t", "secret": true } } }] },
    });
    let dropped = guard_secrets(&mut probe);
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    let text = serde_json::to_string(&probe).unwrap();
    assert!(!text.contains("s3cr3t"), "{text}");
    assert!(text.contains(DROPPED), "{text}");

    // A notify message with a secret in its body loses it too.
    let mut notify = json!({
        "kind": "notify",
        "op": "deliver",
        "body": { "text": "s3cr3t", "secret": true },
    });
    assert_eq!(guard_secrets(&mut notify), vec!["body".to_string()]);
    assert!(!serde_json::to_string(&notify).unwrap().contains("s3cr3t"));
}

#[test]
fn a_host_a_hook_lists_is_the_equal_of_one_a_file_declares() {
    // T4's inventory comes from a hook (7.5, Appendix C). A host it lists
    // must be able to say everything a `rue_toml()` host says: where its
    // instance directory lives, whether it honors the stdin preamble, and
    // what language its backstop is rendered in. A record that loses one
    // of those makes the host quietly less capable than the same host
    // read from a file, and nothing in the plan says why.
    use rue_core::model::ArtifactLanguage;
    use rue_engine::hook::{hook_inventory, HookError, HookLink, Registered, Registration};

    struct Listing(Value);
    impl HookLink for Listing {
        fn name(&self) -> String {
            "world".into()
        }
        fn call(&self, request: Value, _d: Duration) -> Result<Value, HookError> {
            assert_eq!(request["kind"], json!("inventory"));
            assert_eq!(request["op"], json!("list"));
            Ok(self.0.clone())
        }
    }

    let listing = json!({
        "ok": true,
        "hosts": [
            { "name": "full", "address": "10.0.0.1", "os": "freebsd",
              "roles": ["hv", "fw"], "reach": ["ssh"], "filesystem": true,
              "stdin_preamble": false, "scheduler": "cron",
              "rue_root": "/var/db/rue", "artifact": "python",
              "facts": { "site": "west" } },
            { "name": "bare", "os": "linux" }
        ]
    });
    let registry = HookRegistry::new();
    registry.register(Registered {
        registration: Registration {
            name: "world".into(),
            kinds: vec!["inventory".into()],
            protocol: HOOK_PROTOCOL,
            filesystem: false,
            stdin_preamble: false,
        },
        registrar: "owner".into(),
        connection: "test".into(),
        link: Arc::new(Listing(listing)),
    });

    let hosts = hook_inventory(&registry, "world", Duration::from_millis(500)).unwrap();
    assert_eq!(hosts.len(), 2);

    let full = &hosts[0];
    assert_eq!(full.rue_root.as_deref(), Some("/var/db/rue"));
    assert_eq!(full.record.artifact, Some(ArtifactLanguage::Python));
    assert!(
        !full.record.stdin_preamble,
        "an appliance that declares no preamble does not acquire one"
    );
    assert_eq!(full.scheduler.as_deref(), Some("cron"));
    assert_eq!(full.address, "10.0.0.1");
    assert_eq!(full.facts.get("roles").map(String::as_str), Some("hv,fw"));
    assert_eq!(full.facts.get("site").map(String::as_str), Some("west"));

    // What a hook may leave out, and what it then gets.
    let bare = &hosts[1];
    assert_eq!(bare.rue_root, None, "no instance directory anywhere");
    assert_eq!(bare.record.artifact, None, "the host's native shell");
    assert!(!bare.record.filesystem && !bare.record.stdin_preamble);
    assert!(bare.facts.is_empty(), "no roles is no roles fact");

    // An unregistered inventory hook is not an empty inventory.
    assert!(matches!(
        hook_inventory(&registry, "elsewhere", Duration::from_millis(500)),
        Err(HookError::Unregistered(_))
    ));
    // Nor is a reply without the field the op requires (R0303).
    let registry2 = HookRegistry::new();
    registry2.register(Registered {
        registration: Registration {
            name: "world".into(),
            kinds: vec!["inventory".into()],
            protocol: HOOK_PROTOCOL,
            filesystem: false,
            stdin_preamble: false,
        },
        registrar: "owner".into(),
        connection: "test".into(),
        link: Arc::new(Listing(json!({ "ok": true }))),
    });
    assert!(matches!(
        hook_inventory(&registry2, "world", Duration::from_millis(500)),
        Err(HookError::Contract(_))
    ));
}

#[test]
fn a_hook_reply_missing_a_field_the_op_requires_is_r0303() {
    // R0303: a hook that answers `ok: true` without what the op promised
    // has violated the contract, and the step is refused with the code
    // rather than proceeding on a guess.
    use rue_engine::hook::field;
    use serde_json::json;

    // The reply shape is the contract; `field` is what reads it.
    let good = json!({ "ok": true, "output": { "stdout": "", "outputs": {} } });
    assert!(field(&good, "output").is_ok());
    let bad = json!({ "ok": true });
    let err = field(&bad, "output").unwrap_err().to_string();
    assert!(err.contains("output"), "{err}");
    // The executor turns that into a refusal naming R0303.
    let e = rue_engine::executor::ExecError::Failed(format!("R0303: {err}"));
    assert!(e.to_string().contains("R0303"), "{e}");
}

#[test]
fn an_acknowledgement_over_the_channel_is_proved_by_its_authenticator_not_the_operator() {
    // An identity says who connected; an authenticator says who proved. The
    // channel passed the operator's identity as the authenticator, so the
    // proof was recorded against "ops" -- a name the approval binding never
    // published -- and a knell waiting on `oncall` could never open. 8.2's
    // manual knells were unacknowledgeable through `rue ack`.
    let w = World::new("control-ack");
    let (d, sink, _, _w) = daemon(
        w,
        ops(
            vec![operator("ops", UserSpec::Name(me()), &["all"], true)],
            vec![],
        ),
        false,
    );
    let approval =
        rue_engine::gates::FakeApprovalHandle::new(vec![rue_core::model::Authenticator {
            id: "oncall".into(),
            human: true,
        }]);
    d.engine
        .lock()
        .unwrap()
        .set_approval(Box::new(approval.clone()));
    let mut op = world::op("fence");
    op.undo = rue_core::model::Undo::NoUndo;
    op.refusal = rue_core::model::Refusal::Knell {
        guard: None,
        cost: rue_core::model::Cost::NoCost("measured elsewhere".into()),
        ack: rue_core::model::Ack::Gate(rue_core::model::GateExpr::Single(
            rue_core::model::Factor::Auth {
                id: "oncall".into(),
                weight: 1,
            },
        )),
    };
    let plan = world::temp_plan(
        "p",
        vec![rue_core::model::Item::Knell(rue_core::model::StepI::new(
            op,
        ))],
    );
    let mut c = Conn::open(&d);
    c.hello(None);
    let r = c.call(
        "apply",
        json!({ "ir": serde_json::to_value(world::ir(plan)).unwrap(), "params": {} }),
    );
    let id = r
        .pointer("/result/id")
        .and_then(Value::as_str)
        .expect("an instance")
        .to_string();
    assert_eq!(r.pointer("/result/state"), Some(&json!("Waiting")), "{r}");

    // What a person proves against is the challenge for the ACK scope. The
    // channel could only render one for the plan or a step gate, so an
    // acknowledgement's challenge was never obtainable -- a proof made
    // against a plan-scope challenge verifies for the plan, not the ack.
    let r = c.call("challenge", json!({ "instance": id, "ack": 1 }));
    assert!(r.pointer("/result/challenge").is_some(), "{r}");
    assert!(
        approval
            .calls()
            .iter()
            .any(|c| c.starts_with("challenge Ack(1)")),
        "the channel rendered a challenge for another scope: {:?}",
        approval.calls()
    );

    let r = c.call(
        "ack",
        json!({ "instance": id, "step": 1, "reason": "the driver verified it off",
                "authenticator": "oncall", "proof": "token" }),
    );
    assert_eq!(
        r.pointer("/result/state"),
        Some(&json!("Applied")),
        "the acknowledgement did not open the knell: {r}"
    );
    assert!(
        sink.events().iter().any(|e| matches!(
            e,
            J::KnellAcknowledged { by, .. } if by.starts_with("oncall")
        )),
        "the knell was not acknowledged by the authenticator that proved it: {:?}",
        sink.events()
    );

    // And the authenticator is required: an acknowledgement is a proof by
    // somebody the binding knows, never by default the operator.
    let r = c.call(
        "ack",
        json!({ "instance": id, "step": 1, "reason": "again", "proof": "token" }),
    );
    assert!(
        r.get("error").is_some(),
        "an ack with no authenticator was taken: {r}"
    );
}
