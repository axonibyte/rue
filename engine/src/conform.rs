//! `rue sdk-conform`: the daemon's side of docs/sdk-conformance.md.
//!
//! One hook, spawned and driven through every op of 7.5 it registered for,
//! judged against the fixed world that document spells out. It needs no
//! daemon, no store and no plan: a [`LineLink`] over the child's stdio is
//! the whole apparatus, which is also what makes this a fair test -- the
//! frames a hook sees here are the frames `rued` sends.
//!
//! Every reply is judged twice: once against the case's own expected
//! answer, and once against the op's row in `rue-hook-proto` -- the id
//! comes back, `ok` is a boolean, an `ok: true` carries every field the op
//! requires, and it all arrives inside the deadline. The second judgement
//! is what an SDK is really for: a hook written on one cannot answer
//! `ok: true` without a required field, because the SDK builds the reply.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rue_hook_proto::{Op, Registration};
use serde_json::{json, Value};

use crate::hook::{spawn_stdio_hook, HookError, HookLink, LineLink};

/// One case's verdict.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// `kind.op`, or `registration` for the handshake.
    pub op: String,
    /// What this case asked of the hook.
    pub case: String,
    pub passed: bool,
    /// What happened, in the words an operator would want.
    pub detail: String,
}

/// What a run of the suite found.
#[derive(Debug, Clone)]
pub struct Report {
    pub name: String,
    pub registration: Registration,
    pub outcomes: Vec<Outcome>,
}

impl Report {
    pub fn failed(&self) -> usize {
        self.outcomes.iter().filter(|o| !o.passed).count()
    }
    pub fn passed(&self) -> usize {
        self.outcomes.iter().filter(|o| o.passed).count()
    }
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "registration": {
                "kinds": self.registration.kinds,
                "protocol": self.registration.protocol,
                "filesystem": self.registration.filesystem,
                "stdin_preamble": self.registration.stdin_preamble,
            },
            "passed": self.passed(),
            "failed": self.failed(),
            "cases": self.outcomes.iter().map(|o| json!({
                "op": o.op, "case": o.case, "passed": o.passed, "detail": o.detail,
            })).collect::<Vec<_>>(),
        })
    }
}

/// The suite could not be run at all -- distinct from a hook that ran and
/// failed cases, which is a [`Report`].
#[derive(Debug)]
pub struct Unstartable(pub String);

impl std::fmt::Display for Unstartable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Spawn `command` as a stdio hook registering as `name`, and run the
/// suite against it.
pub fn conform(name: &str, command: &str, deadline: Duration) -> Result<Report, Unstartable> {
    let mut hook = spawn_stdio_hook(name, command).map_err(|e| Unstartable(e.to_string()))?;
    let registration = hook.registration.clone();
    let mut outcomes = vec![registration_case(name, &registration)];
    hook.acknowledge().map_err(|e| Unstartable(e.to_string()))?;

    let link = hook.link.clone();
    let stdout = hook
        .take_stdout()
        .ok_or_else(|| Unstartable("no stdout".into()))?;
    let pumping = link.clone();
    let pump = thread::spawn(move || pumping.pump(stdout));

    {
        let serves = |k: &str| registration.kinds.iter().any(|s| s == k);
        let mut d = Driver {
            link: hook.link.clone(),
            deadline,
            out: &mut outcomes,
        };
        if serves("journal") {
            journal_cases(&mut d);
        }
        if serves("inventory") {
            inventory_cases(&mut d);
        }
        if serves("execute") {
            execute_cases(&mut d);
        }
        if serves("probe") {
            probe_cases(&mut d);
        }
        if serves("approval") {
            approval_cases(&mut d);
        }
        if serves("secrets") {
            secrets_cases(&mut d);
        }
        if serves("notify") {
            notify_cases(&mut d);
        }
        if serves("scheduler") {
            scheduler_cases(&mut d);
        }
        unknown_op_case(&mut d);
    }

    // Close its stdin and let it end, then read the pump to its end: the
    // hook may not be the process we spawned, and killing that one leaves
    // whatever is really serving holding the pipe.
    let _ = hook.shutdown(Duration::from_secs(5));
    let _ = pump.join();
    Ok(Report {
        name: name.to_string(),
        registration,
        outcomes,
    })
}

fn registration_case(name: &str, r: &Registration) -> Outcome {
    let mut why = Vec::new();
    if r.name != name {
        why.push(format!("registers as `{}`, not `{name}`", r.name));
    }
    if r.protocol != rue_hook_proto::HOOK_PROTOCOL {
        why.push(format!(
            "names protocol {}, not {}",
            r.protocol,
            rue_hook_proto::HOOK_PROTOCOL
        ));
    }
    if r.kinds.is_empty() {
        why.push("registers for no kind at all".to_string());
    }
    for k in &r.kinds {
        if !rue_hook_proto::KINDS.contains(&k.as_str()) {
            why.push(format!("`{k}` is not a kind of this protocol"));
        }
    }
    Outcome {
        op: "registration".into(),
        case: "the first line is a registration this protocol admits".into(),
        passed: why.is_empty(),
        detail: if why.is_empty() {
            format!("serves {}", r.kinds.join(", "))
        } else {
            why.join("; ")
        },
    }
}

struct Driver<'a> {
    link: Arc<LineLink>,
    deadline: Duration,
    out: &'a mut Vec<Outcome>,
}

impl Driver<'_> {
    /// Send a request, apply the universal checks, then the case's own.
    fn case<F>(&mut self, case: &str, request: Value, judge: F)
    where
        F: FnOnce(&Value) -> Result<String, String>,
    {
        let kind = request["kind"].as_str().unwrap_or("").to_string();
        let op_name = request["op"].as_str().unwrap_or("").to_string();
        let op = format!("{kind}.{op_name}");
        let outcome = match self.link.call(request, self.deadline) {
            Ok(reply) => match required_fields(&kind, &op_name, &reply) {
                Err(missing) => (false, missing),
                Ok(()) => match judge(&reply) {
                    Ok(detail) => (true, detail),
                    Err(why) => (false, why),
                },
            },
            Err(HookError::Silent) => (
                false,
                format!(
                    "no reply within {}ms: the engine reads that as Silent and refuses the step \
                     with nothing to tell the operator",
                    self.deadline.as_millis()
                ),
            ),
            Err(HookError::Refused(r)) => (false, format!("refused: {r}")),
            Err(HookError::Contract(m)) => (false, format!("R0303: {m}")),
            Err(e) => (false, e.to_string()),
        };
        self.out.push(Outcome {
            op,
            case: case.to_string(),
            passed: outcome.0,
            detail: outcome.1,
        });
    }

    /// A case whose expected answer is a refusal or a violation, judged on
    /// the error rather than the reply.
    fn provocation<F>(&mut self, case: &str, request: Value, judge: F)
    where
        F: FnOnce(Result<Value, HookError>) -> Result<String, String>,
    {
        let kind = request["kind"].as_str().unwrap_or("").to_string();
        let op_name = request["op"].as_str().unwrap_or("").to_string();
        let answered = self.link.call(request, self.deadline);
        let (passed, detail) = match judge(answered) {
            Ok(d) => (true, d),
            Err(w) => (false, w),
        };
        self.out.push(Outcome {
            op: format!("{kind}.{op_name}"),
            case: case.to_string(),
            passed,
            detail,
        });
    }
}

/// The universal check: an `ok: true` carries every field its row requires.
fn required_fields(kind: &str, op: &str, reply: &Value) -> Result<(), String> {
    let Some(row) = Op::find(kind, op) else {
        return Ok(());
    };
    let missing: Vec<&str> = row
        .required_reply
        .iter()
        .filter(|f| reply.get(**f).is_none())
        .copied()
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "R0303: answered ok without `{}`, which {kind}.{op} requires",
            missing.join("`, `")
        ))
    }
}

fn want(reply: &Value, pointer: &str, expected: Value) -> Result<(), String> {
    let got = reply.pointer(pointer);
    if got == Some(&expected) {
        Ok(())
    } else {
        Err(format!(
            "{pointer} is {}, not {expected}",
            got.map(Value::to_string).unwrap_or_else(|| "absent".into())
        ))
    }
}

// ---------------------------------------------------------------------------
// The cases of docs/sdk-conformance.md, one function per kind.

fn journal_cases(d: &mut Driver) {
    let entry = json!({ "seq": 1, "plan": "conform", "event": "Checked" });
    d.case(
        "an entry is acknowledged",
        {
            let mut v = rue_hook_proto::request::req("journal", "append");
            v["entry"] = entry;
            v
        },
        |_| Ok("acknowledged".into()),
    );
}

fn inventory_cases(d: &mut Driver) {
    d.case(
        "two hosts, and an omitted field takes its default rather than failing",
        rue_hook_proto::request::inventory_list(),
        |reply| {
            let hosts: Vec<rue_hook_proto::InventoryHost> =
                serde_json::from_value(reply["hosts"].clone())
                    .map_err(|e| format!("hosts do not parse as Appendix C records: {e}"))?;
            if hosts.len() != 2 {
                return Err(format!("{} hosts, not 2", hosts.len()));
            }
            let full = &hosts[0];
            let bare = &hosts[1];
            if full.name != "conform-full" || bare.name != "conform-bare" {
                return Err(format!(
                    "hosts are `{}` and `{}`, not `conform-full` and `conform-bare`",
                    full.name, bare.name
                ));
            }
            if full.rue_root.as_deref() != Some("/var/db/rue") {
                return Err(
                    "conform-full lost its rue_root: a host with none can hold no \
                            instance directory"
                        .into(),
                );
            }
            if full.artifact != Some(rue_core::model::ArtifactLanguage::Python) {
                return Err("conform-full lost its artifact language".into());
            }
            if full.stdin_preamble != Some(false) {
                return Err("conform-full's stdin_preamble is not the false it declared".into());
            }
            if full.scheduler.as_deref() != Some("cron") {
                return Err("conform-full lost its scheduler".into());
            }
            if full.roles != vec!["a".to_string(), "b".to_string()] {
                return Err("conform-full lost its roles".into());
            }
            if bare.rue_root.is_some() || bare.artifact.is_some() || bare.filesystem {
                return Err("conform-bare invented a field it did not declare".into());
            }
            Ok("both hosts, every field carried".into())
        },
    );
}

fn execute_cases(d: &mut Driver) {
    use rue_hook_proto::{request, RPrim, Resolved};

    let body = vec![RPrim::Run {
        cmd: Resolved::plain("conformance"),
        env: vec![(
            "PW".into(),
            Resolved {
                text: "conformance-secret".into(),
                secret: true,
            },
        )],
        stdin: None,
    }];
    d.case(
        "a run reaches the handler with its secret intact, and the outputs come back",
        request::execute_run("conform-full", "conform-1", &body),
        |reply| {
            want(reply, "/output/stdout", json!("ran 1 primitive\n"))?;
            want(reply, "/output/outputs/echo", json!("conformance"))?;
            want(reply, "/output/outputs/secret", json!("conformance-secret")).map_err(|e| {
                format!(
                    "{e}. execute.run carries a secret in both directions (7.5); an SDK that \
                     scrubs it on the way in, or cannot reach it, fails here"
                )
            })?;
            Ok("the body, its secret and its outputs all crossed".into())
        },
    );
    d.case(
        "a fact that is there comes back",
        request::execute_read_fact("conform-full", "file:/conformance/present"),
        |reply| {
            want(reply, "/content", json!("present\n"))?;
            Ok("content returned".into())
        },
    );
    d.case(
        "a fact that is not there is an absent field, not an empty one",
        request::execute_read_fact("conform-full", "file:/conformance/absent"),
        |reply| match reply.get("content") {
            None => Ok("absent, as it should be".into()),
            Some(v) => Err(format!(
                "answered content {v}: an empty string is a file that exists and is empty, \
                 which is a different fact"
            )),
        },
    );
    d.case(
        "the bootstrap state is the five flags of 7.7",
        request::execute_op("bootstrap_state", "conform-full", None),
        |reply| {
            let st: rue_hook_proto::BootstrapState = serde_json::from_value(reply["state"].clone())
                .map_err(|e| format!("state does not parse: {e}"))?;
            if st.ready() {
                Ok("ready".into())
            } else {
                Err(format!(
                    "{st:?} is not the all-true state the contract fixes"
                ))
            }
        },
    );
    d.case(
        "an instance directory listing",
        request::execute_op("instance_dir_list", "conform-full", None),
        |reply| {
            let dirs: Vec<rue_hook_proto::InstanceDirState> =
                serde_json::from_value(reply["dirs"].clone())
                    .map_err(|e| format!("dirs do not parse: {e}"))?;
            match dirs.first() {
                Some(x) if x.instance == "conform-1" && x.armed && !x.fired && x.modes_ok => {
                    Ok("one armed, unfired directory".into())
                }
                other => Err(format!("{other:?} is not the contract's one entry")),
            }
        },
    );
    d.case(
        "a staged file is read back",
        request::execute_get_file("conform-full", "conform-1", "deadline"),
        |reply| {
            want(reply, "/content", json!("1700000000\n"))?;
            Ok("content returned".into())
        },
    );
    for (op, request) in [
        (
            "instance_dir_create",
            request::execute_op("instance_dir_create", "conform-full", Some("conform-1")),
        ),
        (
            "instance_dir_remove",
            request::execute_op("instance_dir_remove", "conform-full", Some("conform-1")),
        ),
        (
            "put_file",
            request::execute_put_file("conform-full", "conform-1", "markers/1", "owned", 0o640),
        ),
        (
            "replace_file",
            request::execute_replace_file("conform-full", "conform-1", "deadline", "1700000000\n"),
        ),
        (
            "remove_file",
            request::execute_remove_file("conform-full", "conform-1", "markers/1"),
        ),
        (
            "host_lock",
            request::execute_op("host_lock", "conform-full", None),
        ),
    ] {
        d.case(&format!("{op} is acknowledged"), request, |_| {
            Ok("acknowledged".into())
        });
    }
    // The one op a hook may decline outright.
    d.provocation(
        "the clock is answered, or declined -- both conform",
        request::execute_op("clock", "conform-full", None),
        |answered| match answered {
            Ok(reply) => match reply.get("epoch_s").and_then(Value::as_u64) {
                Some(1_700_000_000) => Ok("answered 1700000000".into()),
                Some(other) => Err(format!("answered {other}, not the contract's 1700000000")),
                None => Err("answered ok without epoch_s (R0303)".into()),
            },
            Err(HookError::Refused(_)) => {
                Ok("declined, so the engine records that no skew probe is possible here".into())
            }
            Err(e) => Err(format!("{e}")),
        },
    );
}

fn probe_cases(d: &mut Driver) {
    use rue_hook_proto::request::probe_observe;
    for (probe, text, tri) in [
        ("conform-yes", "yes", "yes"),
        ("conform-no", "no", "no"),
        ("conform-unknown", "", "unknown"),
    ] {
        d.case(
            &format!("`{probe}` observes {tri}"),
            probe_observe("conform-full", probe),
            move |reply| {
                want(reply, "/fact/text", json!(text))?;
                want(reply, "/fact/tri", json!(tri))?;
                Ok(tri.to_string())
            },
        );
    }
    d.provocation(
        "a refusal names its reason and is not a silence",
        probe_observe("conform-full", "conform-refuse"),
        |answered| match answered {
            Err(HookError::Refused(r)) if !r.trim().is_empty() => Ok(format!("refused: {r}")),
            Err(HookError::Refused(_)) => Err("refused with an empty reason".into()),
            Ok(_) => Err("answered ok where the contract asks for a refusal".into()),
            Err(e) => Err(format!("{e}")),
        },
    );
    d.provocation(
        "an ok without a field the op requires is caught as R0303",
        probe_observe("conform-full", "conform-missing-field"),
        |answered| match answered {
            Ok(reply) => match required_fields("probe", "observe", &reply) {
                Err(_) => Ok("caught: the engine would refuse the step with R0303".into()),
                Ok(()) => {
                    Err("the hook answered the field; the provocation is not implemented".into())
                }
            },
            Err(e) => Err(format!("{e}")),
        },
    );
    d.provocation(
        "a reply with no boolean ok is R0303",
        probe_observe("conform-full", "conform-no-ok"),
        |answered| match answered {
            Err(HookError::Contract(m)) => Ok(format!("caught: {m}")),
            Ok(_) => {
                Err("the hook answered a boolean ok; the provocation is not implemented".into())
            }
            Err(e) => Err(format!("{e}")),
        },
    );
    d.provocation(
        "no reply at all is Silent at the deadline",
        probe_observe("conform-full", "conform-silent"),
        |answered| match answered {
            Err(HookError::Silent) => {
                Ok("Silent, which the engine treats as a refusal of the step".into())
            }
            Ok(_) => Err("the hook answered; the provocation is not implemented".into()),
            Err(e) => Err(format!("{e}")),
        },
    );
}

fn approval_cases(d: &mut Driver) {
    use rue_hook_proto::request::{approval_authenticators, approval_challenge, approval_verify};
    const DIGEST: &str = "00112233445566778899aabbccddeeff";
    const OTHER: &str = "ffeeddccbbaa99887766554433221100";

    d.case(
        "the authenticators are published with their human flags",
        approval_authenticators(),
        |reply| {
            let auths = reply["authenticators"]
                .as_array()
                .ok_or_else(|| "authenticators is not an array".to_string())?;
            let ids: Vec<&str> = auths.iter().filter_map(|a| a["id"].as_str()).collect();
            if ids != vec!["conform-human", "conform-machine"] {
                return Err(format!("{ids:?} is not the contract's pair"));
            }
            if auths[0]["human"] != json!(true) || auths[1]["human"] != json!(false) {
                return Err("the human flags are not as the contract fixes them".into());
            }
            Ok("two authenticators, one human".into())
        },
    );
    d.case(
        "the challenge names the request it is for",
        approval_challenge(
            "conform-1",
            DIGEST,
            json!("plan"),
            json!({ "plan": "conform" }),
        ),
        |reply| {
            let c = reply["challenge"].as_str().unwrap_or("");
            if c.contains(DIGEST) {
                Ok("names the digest".into())
            } else {
                Err(format!(
                    "`{c}` does not name the digest it is for, so a person approving it cannot \
                     tell which request they are approving"
                ))
            }
        },
    );
    let proof = format!("{DIGEST}/plan");
    d.case(
        "the proof made for this request and scope verifies",
        approval_verify("conform-1", DIGEST, json!("plan"), "conform-human", &proof),
        |reply| {
            want(reply, "/verified", json!(true))?;
            Ok("verified".into())
        },
    );
    d.case(
        "the same proof does not verify for another scope",
        approval_verify(
            "conform-1",
            DIGEST,
            json!({ "step": 1 }),
            "conform-human",
            &proof,
        ),
        |reply| {
            want(reply, "/verified", json!(false)).map_err(|e| {
                format!(
                    "{e}. A proof is bound to one scope (5.11): one made for the plan must not \
                     satisfy a step"
                )
            })?;
            Ok("refused, as a replay across scopes must be".into())
        },
    );
    d.case(
        "the same proof does not verify for another request",
        approval_verify("conform-1", OTHER, json!("plan"), "conform-human", &proof),
        |reply| {
            want(reply, "/verified", json!(false)).map_err(|e| {
                format!(
                    "{e}. A proof is bound to one digest (5.11): one made for another request \
                     must not satisfy this one"
                )
            })?;
            Ok("refused, as a replay across requests must be".into())
        },
    );
}

fn secrets_cases(d: &mut Driver) {
    use rue_hook_proto::request::{secrets_deliver, secrets_resolve};
    d.case(
        "a reference resolves to its value",
        secrets_resolve("conform"),
        |reply| {
            want(reply, "/value", json!("conformance-resolved-secret"))?;
            Ok("resolved".into())
        },
    );
    d.case(
        "a delivery is accepted with a receipt to journal in place of the value",
        secrets_deliver("conform-1", "conform", "conformance-delivered-secret"),
        |reply| {
            want(reply, "/accepted", json!(true))?;
            want(reply, "/receipt", json!("receipt-conform"))?;
            Ok("accepted".into())
        },
    );
    d.case(
        "declining a delivery is an answer, not a refusal",
        secrets_deliver("conform-1", "unwanted", "conformance-delivered-secret"),
        |reply| {
            want(reply, "/accepted", json!(false)).map_err(|e| {
                format!(
                    "{e}. The engine offers a declined secret to the next acceptor, so \
                         declining is `ok: true` with `accepted: false`"
                )
            })?;
            Ok("declined".into())
        },
    );
}

fn notify_cases(d: &mut Driver) {
    d.case(
        "a notification is acknowledged",
        rue_hook_proto::request::notify_deliver("waiting", "conform", "a plan is waiting"),
        |_| Ok("acknowledged".into()),
    );
}

fn scheduler_cases(d: &mut Driver) {
    use rue_hook_proto::request::scheduler_op;
    for op in ["install", "arm", "rearm", "disarm"] {
        d.case(
            &format!("{op} is acknowledged"),
            scheduler_op(
                op,
                "conform-full",
                "conform-present.sh",
                Some(1_700_000_000),
            ),
            |_| Ok("acknowledged".into()),
        );
    }
    for (artifact, expected, what) in [
        ("conform-present.sh", json!(true), "present"),
        ("conform-absent.sh", json!(false), "absent"),
        ("conform-unheard-of.sh", json!("unknown"), "unknown"),
    ] {
        d.case(
            &format!("present reports {what}"),
            scheduler_op("present", "conform-full", artifact, None),
            move |reply| {
                want(reply, "/present", expected).map_err(|e| {
                    format!(
                        "{e}. The engine never reads unknown as absence: answering `false` where \
                         you do not know makes it install again, which is destructive when wrong"
                    )
                })?;
                Ok(what.to_string())
            },
        );
    }
}

fn unknown_op_case(d: &mut Driver) {
    d.provocation(
        "an op this protocol has no row for is refused, never met with silence",
        json!({ "kind": "execute", "op": "reboot", "host": "conform-full" }),
        |answered| match answered {
            Err(HookError::Refused(_)) => Ok("refused by name".into()),
            Err(HookError::Silent) => Err(
                "silence: the engine cannot tell `I do not serve that` from `I am gone`, and \
                 the operator gets a step that did not happen and no reason"
                    .into(),
            ),
            Ok(_) => Err("answered ok to an op that does not exist".into()),
            Err(e) => Err(format!("{e}")),
        },
    );
}
