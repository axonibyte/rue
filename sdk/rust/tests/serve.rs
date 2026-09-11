//! The SDK against the frames of docs/hook-protocol.md: what it registers
//! for, what it answers, what it refuses, and the two ways a hook is
//! reached. A reply this crate builds must carry the fields its op declares
//! (R0303 at the far end otherwise), so every op is driven here and its
//! reply checked against `rue-hook-proto`'s own table rather than against a
//! list written out a second time.

use std::io::BufReader;
use std::time::Duration;

use rue_hook_proto::{
    BootstrapState, InstanceDirState, InventoryHost, Observation, Op, Output, RPrim, Resolved,
    HOOK_PROTOCOL, OPS,
};
use rue_hook_sdk::{
    serve_socket, Answer, Approval, Authenticator, ChallengeRequest, Delivery, Execute, Hooks,
    Inventory, Journal, Notify, Presence, Probe, Scheduler, Secrets, ServeOptions, Verdict,
    VerifyRequest,
};
use serde_json::{json, Value};

/// A hook that serves every kind, over a world small enough to assert on.
#[derive(Default)]
struct Everything {
    /// What the last `execute.run` was handed, so a test can prove the
    /// secret arrived intact rather than scrubbed on the way in.
    last_body: Vec<RPrim>,
    slow: Option<Duration>,
}

impl Journal for Everything {
    fn append(&mut self, entry: &Value) -> Answer<()> {
        assert!(entry.is_object() || entry.is_string());
        Ok(())
    }
}

impl Inventory for Everything {
    fn list(&mut self) -> Answer<Vec<InventoryHost>> {
        Ok(vec![serde_json::from_value(json!({
            "name": "site-ctl", "os": "linux", "reach": ["hook"], "roles": ["ctl"]
        }))
        .unwrap()])
    }
}

impl Execute for Everything {
    fn run(&mut self, _host: &str, _instance: &str, body: &[RPrim]) -> Answer<Output> {
        if let Some(d) = self.slow {
            std::thread::sleep(d);
        }
        self.last_body = body.to_vec();
        let mut out = Output {
            stdout: "done\n".into(),
            ..Output::default()
        };
        out.outputs.insert("token".into(), "t-1".into());
        Ok(out)
    }
    fn read_fact(&mut self, _host: &str, shape: &str) -> Answer<Option<String>> {
        Ok(if shape == "file:/etc/present" {
            Some("here".into())
        } else {
            None
        })
    }
    fn bootstrap_state(&mut self, _host: &str) -> Answer<BootstrapState> {
        Ok(BootstrapState {
            rue_root: true,
            group: true,
            instances_dir: true,
            lock: true,
            modes_ok: true,
        })
    }
    fn clock(&mut self, _host: &str) -> Answer<u64> {
        Ok(1_700_000_000)
    }
    fn instance_dir_create(&mut self, _h: &str, _i: &str) -> Answer<()> {
        Ok(())
    }
    fn instance_dir_remove(&mut self, _h: &str, _i: &str) -> Answer<()> {
        Ok(())
    }
    fn instance_dir_list(&mut self, _h: &str) -> Answer<Vec<InstanceDirState>> {
        Ok(vec![InstanceDirState {
            instance: "i-1".into(),
            armed: true,
            fired: false,
            modes_ok: true,
        }])
    }
    fn put_file(&mut self, _h: &str, _i: &str, _r: &str, _c: &str, _m: u32) -> Answer<()> {
        Ok(())
    }
    fn replace_file(&mut self, _h: &str, _i: &str, _r: &str, _c: &str) -> Answer<()> {
        Ok(())
    }
    fn get_file(&mut self, _h: &str, _i: &str, _r: &str) -> Answer<String> {
        Ok("9\n".into())
    }
    fn remove_file(&mut self, _h: &str, _i: &str, _r: &str) -> Answer<()> {
        Ok(())
    }
    fn host_lock(&mut self, _h: &str) -> Answer<()> {
        Ok(())
    }
}

impl Probe for Everything {
    fn observe(&mut self, _host: &str, probe: &str) -> Answer<Observation> {
        Ok(match probe {
            "up" => Observation::yes("up"),
            "down" => Observation::no("down"),
            _ => Observation::unknown(""),
        })
    }
}

impl Approval for Everything {
    fn authenticators(&mut self) -> Answer<Vec<Authenticator>> {
        Ok(vec![Authenticator {
            id: "operator".into(),
            human: true,
        }])
    }
    fn challenge(&mut self, r: &ChallengeRequest) -> Answer<String> {
        Ok(format!("approve {} for {}", r.digest, r.instance))
    }
    fn verify(&mut self, r: &VerifyRequest) -> Answer<Verdict> {
        // The proof is bound to the digest and the scope it was made for:
        // a proof that names another is not this one (5.11).
        let want = format!("{}:{}", r.digest, r.scope);
        Ok(Verdict {
            verified: r.proof == want,
            reason: if r.proof == want {
                String::new()
            } else {
                "the proof names another request or another scope".into()
            },
        })
    }
}

impl Secrets for Everything {
    fn resolve(&mut self, reference: &str) -> Answer<String> {
        Ok(format!("value-of-{reference}"))
    }
    fn deliver(&mut self, _i: &str, label: &str, _v: &str) -> Answer<Delivery> {
        Ok(Delivery {
            accepted: label != "unwanted",
            receipt: format!("receipt-{label}"),
        })
    }
}

impl Notify for Everything {
    fn deliver(&mut self, _l: &str, _s: &str, _b: &str) -> Answer<()> {
        Ok(())
    }
}

impl Scheduler for Everything {
    fn install(&mut self, _h: &str, _a: &str) -> Answer<()> {
        Ok(())
    }
    fn arm(&mut self, _h: &str, _a: &str, _d: Option<u64>) -> Answer<()> {
        Ok(())
    }
    fn rearm(&mut self, _h: &str, _a: &str, _d: Option<u64>) -> Answer<()> {
        Ok(())
    }
    fn disarm(&mut self, _h: &str, _a: &str) -> Answer<()> {
        Ok(())
    }
    fn present(&mut self, _h: &str, artifact: &str) -> Answer<Presence> {
        Ok(match artifact {
            "there.sh" => Presence::Present,
            "gone.sh" => Presence::Absent,
            _ => Presence::Unknown,
        })
    }
}

fn everything() -> Hooks {
    let mut h = Hooks::new();
    h.journal = Some(Box::<Everything>::default());
    h.inventory = Some(Box::<Everything>::default());
    h.execute = Some(Box::<Everything>::default());
    h.probe = Some(Box::<Everything>::default());
    h.approval = Some(Box::<Everything>::default());
    h.secrets = Some(Box::<Everything>::default());
    h.notify = Some(Box::<Everything>::default());
    h.scheduler = Some(Box::<Everything>::default());
    h.filesystem = true;
    h.stdin_preamble = true;
    h
}

/// One request per op, built the way the engine builds them.
fn every_request() -> Vec<Value> {
    let scope = json!({ "step": 1 });
    let mut v = vec![
        json!({ "kind": "journal", "op": "append", "entry": { "event": "Checked" } }),
        rue_hook_proto::request::inventory_list(),
        rue_hook_proto::request::execute_run(
            "h",
            "i",
            &[RPrim::Run {
                cmd: Resolved::plain("login"),
                env: vec![(
                    "PW".into(),
                    Resolved {
                        text: "s3cr3t".into(),
                        secret: true,
                    },
                )],
                stdin: None,
            }],
        ),
        rue_hook_proto::request::execute_read_fact("h", "file:/etc/present"),
        rue_hook_proto::request::execute_put_file("h", "i", "markers/1", "owned", 0o640),
        rue_hook_proto::request::execute_replace_file("h", "i", "deadline", "9"),
        rue_hook_proto::request::execute_get_file("h", "i", "deadline"),
        rue_hook_proto::request::execute_remove_file("h", "i", "markers/1"),
        rue_hook_proto::request::probe_observe("h", "up"),
        rue_hook_proto::request::approval_authenticators(),
        rue_hook_proto::request::approval_challenge("i", "abc", scope.clone(), Value::Null),
        rue_hook_proto::request::approval_verify("i", "abc", scope, "operator", "no"),
        rue_hook_proto::request::secrets_resolve("db_pw"),
        rue_hook_proto::request::secrets_deliver("i", "pw", "s3cr3t"),
        rue_hook_proto::request::notify_deliver("waiting", "s", "b"),
    ];
    for op in ["clock", "bootstrap_state", "host_lock", "instance_dir_list"] {
        v.push(rue_hook_proto::request::execute_op(op, "h", None));
    }
    for op in ["instance_dir_create", "instance_dir_remove"] {
        v.push(rue_hook_proto::request::execute_op(op, "h", Some("i")));
    }
    for op in ["install", "arm", "rearm", "disarm", "present"] {
        v.push(rue_hook_proto::request::scheduler_op(
            op, "h", "there.sh", None,
        ));
    }
    v
}

#[test]
fn every_op_is_answered_with_the_fields_its_row_requires() {
    let mut hooks = everything();
    let mut answered: Vec<(&str, &str)> = Vec::new();
    for (n, mut request) in every_request().into_iter().enumerate() {
        request["id"] = json!(n);
        let reply = hooks.answer(&request);
        let kind = request["kind"].as_str().unwrap();
        let op_name = request["op"].as_str().unwrap();
        assert_eq!(
            reply["id"],
            json!(n),
            "{kind}.{op_name}: the id must come back"
        );
        assert_eq!(
            reply["ok"],
            json!(true),
            "{kind}.{op_name} was refused: {reply}"
        );
        let op = Op::find(kind, op_name).unwrap();
        for f in op.required_reply {
            assert!(
                reply.get(*f).is_some(),
                "{kind}.{op_name} answered without `{f}`, which is R0303 at the engine: {reply}"
            );
        }
        answered.push((op.kind, op.op));
    }
    let rows: Vec<(&str, &str)> = OPS.iter().map(|o| (o.kind, o.op)).collect();
    let mut sorted = answered.clone();
    sorted.sort_unstable();
    let mut want = rows.clone();
    want.sort_unstable();
    assert_eq!(sorted, want, "an op of the protocol went undriven");
}

#[test]
fn a_kind_that_is_not_served_is_a_refusal_that_names_itself_and_never_a_silence() {
    // A hook registering only `notify` still answers everything asked of
    // it: an unanswered request is Silent, which tells the operator
    // nothing, and an unserved op is a refusal that says which.
    let mut hooks = Hooks::new();
    hooks.notify = Some(Box::<Everything>::default());
    assert_eq!(hooks.kinds(), vec!["notify".to_string()]);

    let r = hooks.answer(&json!({ "id": 4, "kind": "probe", "op": "observe", "host": "h" }));
    assert_eq!(r["id"], json!(4));
    assert_eq!(r["ok"], json!(false));
    assert!(
        r["error"].as_str().unwrap().contains("probe.observe"),
        "{r}"
    );

    // An op the protocol has no row for is refused by name too.
    let r = hooks.answer(&json!({ "id": 5, "kind": "execute", "op": "reboot" }));
    assert_eq!(r["ok"], json!(false));
    assert!(
        r["error"].as_str().unwrap().contains("execute.reboot"),
        "{r}"
    );

    // And an absent field the op requires is a refusal, not a panic.
    let mut j = Hooks::new();
    j.journal = Some(Box::<Everything>::default());
    let r = j.answer(&json!({ "id": 6, "kind": "journal", "op": "append" }));
    assert_eq!(r["ok"], json!(false));
    assert!(r["error"].as_str().unwrap().contains("entry"), "{r}");
}

#[test]
fn an_execute_run_receives_its_secrets_and_a_read_fact_may_answer_nothing() {
    let mut hooks = everything();
    let mut request = rue_hook_proto::request::execute_run(
        "h",
        "i",
        &[RPrim::Run {
            cmd: Resolved::plain("login"),
            env: vec![(
                "PW".into(),
                Resolved {
                    text: "s3cr3t".into(),
                    secret: true,
                },
            )],
            stdin: None,
        }],
    );
    request["id"] = json!(1);
    // The one message that carries secrets toward a hook says which
    // primitives hold one, and the handler gets the value itself.
    assert_eq!(request["secrets"]["prim0"], json!(true));
    let reply = hooks.answer(&request);
    assert_eq!(reply["output"]["outputs"]["token"], json!("t-1"));

    // A body a handler prints does not print the secret with it.
    let body: Vec<RPrim> = serde_json::from_value(request["body"].clone()).unwrap();
    let shown = format!("{body:?}");
    assert!(!shown.contains("s3cr3t"), "{shown}");
    assert!(rue_hook_sdk::carries_secret(&body));
    let RPrim::Run { env, .. } = &body[0] else {
        panic!("the body did not survive the round trip")
    };
    assert_eq!(rue_hook_sdk::expose(&env[0].1), "s3cr3t");

    // "No such file" is an absent `content`, which the engine reads as
    // None; an empty string would be a file that exists and is empty.
    let mut miss = rue_hook_proto::request::execute_read_fact("h", "file:/etc/absent");
    miss["id"] = json!(2);
    let reply = hooks.answer(&miss);
    assert_eq!(reply["ok"], json!(true));
    assert!(reply.get("content").is_none(), "{reply}");
}

#[test]
fn a_proof_verifies_only_against_the_digest_and_the_scope_it_was_made_for() {
    let mut hooks = everything();
    let plan = json!({ "plan": true });
    let step = json!({ "step": 1 });
    let good = format!("abc:{plan}");
    let ask = |h: &mut Hooks, digest: &str, scope: &Value, proof: &str| -> Value {
        let mut r =
            rue_hook_proto::request::approval_verify("i", digest, scope.clone(), "operator", proof);
        r["id"] = json!(9);
        h.answer(&r)
    };
    assert_eq!(
        ask(&mut hooks, "abc", &plan, &good)["verified"],
        json!(true)
    );
    assert_eq!(
        ask(&mut hooks, "abc", &step, &good)["verified"],
        json!(false),
        "a plan proof satisfied a step scope"
    );
    assert_eq!(
        ask(&mut hooks, "def", &plan, &good)["verified"],
        json!(false),
        "a proof for one request satisfied another"
    );
}

#[test]
fn the_socket_handshake_registers_for_what_is_served_and_then_answers_requests() {
    let hooks = everything();
    let script = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        json!({ "id": 0, "result": { "ok": true, "proto": 1 } }),
        json!({ "register": { "ok": true, "name": "actuate" } }),
        // An event frame from a subscription, interleaved: skipped.
        json!({ "event": { "plan": "p", "event": "Applied" } }),
        json!({ "id": 1, "kind": "probe", "op": "observe", "host": "h", "probe": "down" }),
        json!({ "id": 2, "kind": "notify", "op": "deliver", "level": "waiting",
                "subject": "s", "body": "b" }),
    );
    let mut r = BufReader::new(script.as_bytes());
    let mut w: Vec<u8> = Vec::new();
    let mut opts = ServeOptions::new("actuate");
    opts.identity = Some("host".into());
    serve_socket(&mut r, &mut w, hooks, opts).unwrap();

    let out: Vec<Value> = String::from_utf8(w)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(out[0]["verb"], json!("hello"));
    assert_eq!(out[0]["identity"], json!("host"));
    let reg = &out[1]["register"];
    assert_eq!(reg["name"], json!("actuate"));
    assert_eq!(reg["protocol"], json!(HOOK_PROTOCOL));
    assert_eq!(reg["filesystem"], json!(true));
    assert_eq!(
        reg["kinds"],
        json!([
            "journal",
            "inventory",
            "execute",
            "probe",
            "approval",
            "secrets",
            "notify",
            "scheduler"
        ]),
        "the registration names what is served and nothing else"
    );
    assert_eq!(out[2]["id"], json!(1));
    assert_eq!(out[2]["fact"]["tri"], json!("no"));
    assert_eq!(out[3]["id"], json!(2));
    assert_eq!(out.len(), 4, "the event frame was answered: {out:?}");
}

#[test]
fn a_line_that_is_not_json_is_skipped_rather_than_ending_the_hook() {
    let mut script = Vec::new();
    for line in [
        json!({ "id": 0, "result": { "ok": true } }).to_string(),
        json!({ "register": { "ok": true, "name": "actuate" } }).to_string(),
        "not json at all".to_string(),
        "[".repeat(10_000),
        r#"{"id": 1, "kind": "notify", "op": "del"#.to_string(),
    ] {
        script.extend_from_slice(line.as_bytes());
        script.push(b'\n');
    }
    script.extend_from_slice(b"\xff\xfe not UTF-8\n");
    script.extend_from_slice(
        format!(
            "{}\n",
            json!({ "id": 2, "kind": "notify", "op": "deliver", "level": "info",
                    "subject": "s", "body": "b" })
        )
        .as_bytes(),
    );
    let mut r = BufReader::new(&script[..]);
    let mut w: Vec<u8> = Vec::new();
    serve_socket(&mut r, &mut w, everything(), ServeOptions::new("actuate")).unwrap();
    let out: Vec<Value> = String::from_utf8(w)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(out.len(), 3, "hello, register, one reply: {out:?}");
    assert_eq!(
        out[2]["id"],
        json!(2),
        "the request after the bad lines was answered"
    );
    assert_eq!(out[2]["ok"], json!(true));
}

#[test]
fn a_refused_registration_ends_the_serve_loop_rather_than_serving_anyway() {
    let script = format!(
        "{}\n{}\n{}\n",
        json!({ "id": 0, "result": { "ok": true } }),
        json!({ "register": { "ok": false, "error": "R0505" } }),
        json!({ "id": 1, "kind": "notify", "op": "deliver" }),
    );
    let mut r = BufReader::new(script.as_bytes());
    let mut w: Vec<u8> = Vec::new();
    let err = serve_socket(&mut r, &mut w, everything(), ServeOptions::new("actuate")).unwrap_err();
    assert!(err.to_string().contains("R0505"), "{err}");
    let lines = String::from_utf8(w).unwrap();
    assert_eq!(
        lines.lines().count(),
        2,
        "it answered a request anyway: {lines}"
    );
}

#[test]
fn a_handler_over_its_budget_refuses_by_name_instead_of_going_silent() {
    // The engine's deadline is not on the wire, so an SDK cannot see it.
    // What it can do is stop its own slowness from reaching the engine as
    // a silence, which says nothing about why the step did not happen.
    let mut hooks = everything();
    hooks.execute = Some(Box::new(Everything {
        slow: Some(Duration::from_millis(40)),
        ..Everything::default()
    }));
    let script = format!(
        "{}\n{}\n{}\n",
        json!({ "id": 0, "result": { "ok": true } }),
        json!({ "register": { "ok": true, "name": "actuate" } }),
        json!({ "id": 1, "kind": "execute", "op": "run", "host": "h", "instance": "i",
                "body": [] }),
    );
    let mut r = BufReader::new(script.as_bytes());
    let mut w: Vec<u8> = Vec::new();
    let mut opts = ServeOptions::new("actuate");
    opts.budget = Some(Duration::from_millis(1));
    serve_socket(&mut r, &mut w, hooks, opts).unwrap();
    let last: Value =
        serde_json::from_str(String::from_utf8(w).unwrap().lines().last().unwrap()).unwrap();
    assert_eq!(last["id"], json!(1));
    assert_eq!(last["ok"], json!(false));
    assert!(last["error"].as_str().unwrap().contains("budget"), "{last}");

    // Without a budget the same handler answers normally: the option
    // bounds a hook's own patience and changes nothing else.
    let mut hooks = everything();
    hooks.execute = Some(Box::new(Everything {
        slow: Some(Duration::from_millis(40)),
        ..Everything::default()
    }));
    let mut r = BufReader::new(script.as_bytes());
    let mut w: Vec<u8> = Vec::new();
    serve_socket(&mut r, &mut w, hooks, ServeOptions::new("actuate")).unwrap();
    let last: Value =
        serde_json::from_str(String::from_utf8(w).unwrap().lines().last().unwrap()).unwrap();
    assert_eq!(last["ok"], json!(true), "{last}");
}
