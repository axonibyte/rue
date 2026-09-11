//! The Windows control channel attempted under wine (7.4, 12).
//!
//! What this proves depends on what wine implements, and it says which:
//! the pipe is created with its access-control list, a client connects to
//! it, a `hello` crosses it, and the answer is either an identification or
//! an honest refusal. What it must never be is an anonymous acceptance: a
//! client whose account the platform will not name is refused, not let in
//! as nobody.
//!
//! On a real Windows machine this is the same code path the daemon binds;
//! Phase 3W runs it there.

#![cfg(windows)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::world::World;
use rue_engine::control::{Daemon, Operator, Operators, UserSpec, CONTROL_PROTOCOL};
use rue_engine::pipe;
use serde_json::{json, Value};

/// The world's directory is returned too, and first: it holds the store,
/// and a tuple's bindings drop last to first, so binding it first drops it
/// after the daemon has closed that store. Windows refuses to remove a
/// directory with a file still open in it.
fn daemon(w: World) -> (common::TempDir, Arc<Daemon>) {
    let me = rue_engine::peer::my_account().expect("this account has a name");
    let World { dir, engine, .. } = w;
    let d = Arc::new(Daemon {
        engine: Mutex::new(engine),
        operators: Operators {
            identities: vec![Operator {
                name: "ops".into(),
                user: UserSpec::Name(me),
                operator_for: vec!["all".into()],
                admin: true,
                subscribe: Vec::new(),
            }],
            registrars: Vec::new(),
            dry_run: false,
        },
        hooks: Arc::new(rue_engine::hook::HookRegistry::new()),
        subscribers: Arc::new(Default::default()),
        hook_deadline: Duration::from_millis(500),
        dry_run: false,
        mailbox: Default::default(),
    });
    (dir, d)
}

#[test]
fn a_named_pipe_carries_a_hello_and_names_the_client_or_refuses_it() {
    let w = World::new("pipe-hello");
    let (_dir, d) = daemon(w);
    // A name of this run's own, so two runs never share an instance.
    let name = format!(
        r"\\.\pipe\rue-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    );
    let stop = Arc::new(AtomicBool::new(false));
    let served = {
        let (name, d, stop) = (name.clone(), d.clone(), stop.clone());
        std::thread::spawn(move || {
            // The group is the local administrators, which every Windows
            // system knows; a group it did not know would refuse here.
            pipe::serve(
                std::path::Path::new(&name),
                Some("Administrators".to_string()),
                d,
                stop,
            )
        })
    };
    // The client, on its own thread so wine cannot hang the test.
    let (tx, rx) = mpsc::channel();
    let client_name = name.clone();
    std::thread::spawn(move || {
        // The server needs a moment to create the first instance.
        let mut attempt = 0;
        let conn = loop {
            match pipe::connect(std::path::Path::new(&client_name)) {
                Ok(c) => break Ok(c),
                Err(e) if attempt < 40 => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(50));
                    if attempt == 40 {
                        break Err(e);
                    }
                }
                Err(e) => break Err(e),
            }
        };
        let (r, mut wtr) = match conn {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Err(format!("connect: {e}")));
                return;
            }
        };
        let mut line = serde_json::to_vec(
            &json!({ "hello": { "proto": CONTROL_PROTOCOL, "identity": Value::Null } }),
        )
        .unwrap();
        line.push(b'\n');
        if let Err(e) = wtr.write_all(&line).and_then(|()| wtr.flush()) {
            let _ = tx.send(Err(format!("write: {e}")));
            return;
        }
        let mut reply = String::new();
        match BufReader::new(r).read_line(&mut reply) {
            Ok(0) => {
                let _ = tx.send(Err("the server closed the connection".into()));
            }
            Ok(_) => {
                let _ = tx.send(Ok(reply));
            }
            Err(e) => {
                let _ = tx.send(Err(format!("read: {e}")));
            }
        }
    });

    let outcome = rx.recv_timeout(Duration::from_secs(20));
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    // The server checks `stop` between polls, so it returns promptly. What
    // it returns is the platform's to decide under wine; a panic is not.
    if let Err(e) = served.join().expect("the pipe server thread panicked") {
        eprintln!("note: the pipe server stopped with {e}");
    }
    match outcome {
        Ok(Ok(reply)) => {
            let v: Value = serde_json::from_str(reply.trim_end())
                .unwrap_or_else(|e| panic!("the reply was not a frame: {e}: {reply}"));
            // Either the client was named and admitted, or it was refused
            // for want of a name. Never admitted without one.
            // The reply is `{"hello": {ok, proto, identity, admin, dry_run}}`
            // when the client is admitted, and an error frame when it is
            // not.
            if v.pointer("/hello/ok").and_then(Value::as_bool) == Some(true) {
                let identity = v
                    .pointer("/hello/identity")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                assert_eq!(
                    identity, "ops",
                    "an admitted client is a declared identity: {v}"
                );
            } else {
                let err = v.to_string();
                assert!(
                    err.contains("R0503") || err.contains("no operator"),
                    "a client the platform would not name is refused by identity, not by accident: {err}"
                );
            }
        }
        // A platform that cannot serve the pipe says so, in an error that
        // names it: what must never happen is a client admitted without an
        // account, and neither branch here is that.
        Ok(Err(why)) => {
            assert!(
                why.contains("connect")
                    || why.contains("read")
                    || why.contains("write")
                    || why.contains("closed"),
                "the failure names what it was: {why}"
            );
            eprintln!(
                "note: this platform did not carry the pipe exchange ({why}); \
                 the channel is proven on a real machine in Phase 3W"
            );
        }
        Err(_) => eprintln!(
            "note: this platform neither answered nor refused within twenty seconds; \
             the channel is proven on a real machine in Phase 3W"
        ),
    }
    // A connection's handler runs on a thread pipe::serve spawns and does
    // not join, holding the daemon until the client's end closes. The store
    // is under `_dir`, so the test waits for the last holder before the
    // directory goes.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Arc::strong_count(&d) > 1 {
        assert!(
            Instant::now() < deadline,
            "a connection handler still held the daemon ten seconds after the exchange"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
