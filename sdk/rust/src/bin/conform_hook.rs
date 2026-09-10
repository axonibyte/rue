//! The reference conformance hook: docs/sdk-conformance.md's fixed world,
//! served over stdio, which is what `rue sdk-conform` is pointed at to
//! judge this SDK.
//!
//! It is also the worked example the other SDKs' conformance hooks are
//! written to match, so it is written the way an embedder would write one:
//! a struct per kind, the traits implemented on it, and the SDK asked to
//! build every reply.
//!
//! The exception is the four provocations of the `probe` kind, which
//! deliberately violate the protocol. Those cannot go through the SDK,
//! because the SDK will not emit a malformed reply -- `Hooks::answer`
//! builds replies from the op's own row, so an `ok: true` without a
//! required field is not expressible. That is the guarantee the SDK exists
//! for, so the hook drops to the wire for exactly those four and says so
//! here rather than weakening the serve loop to allow them.

use std::io::{BufReader, Write};

use rue_hook_sdk::proto::{
    BootstrapState, InstanceDirState, InventoryHost, Observation, Output, RPrim, Registration,
    HOOK_PROTOCOL,
};
use rue_hook_sdk::{
    read_frame, write_frame, Answer, Approval, Authenticator, ChallengeRequest, Delivery, Execute,
    Hooks, Inventory, Journal, Notify, Presence, Probe, Refusal, Scheduler, Secrets, Verdict,
    VerifyRequest,
};
use serde_json::{json, Value};

struct World;

impl Journal for World {
    fn append(&mut self, _entry: &Value) -> Answer<()> {
        Ok(())
    }
}

impl Inventory for World {
    fn list(&mut self) -> Answer<Vec<InventoryHost>> {
        let full: InventoryHost = serde_json::from_value(json!({
            "name": "conform-full",
            "address": "198.51.100.7",
            "os": "freebsd",
            "roles": ["a", "b"],
            "reach": ["hook"],
            "filesystem": true,
            "stdin_preamble": false,
            "scheduler": "cron",
            "rue_root": "/var/db/rue",
            "artifact": "python",
            "facts": { "site": "west" }
        }))
        .expect("the contract's full host");
        // Only what Appendix C requires: everything else takes its default.
        let bare: InventoryHost =
            serde_json::from_value(json!({ "name": "conform-bare", "os": "linux" }))
                .expect("the contract's bare host");
        Ok(vec![full, bare])
    }
}

impl Execute for World {
    fn run(&mut self, _host: &str, _instance: &str, body: &[RPrim]) -> Answer<Output> {
        let mut out = Output {
            stdout: format!("ran {} primitive\n", body.len()),
            ..Output::default()
        };
        let RPrim::Run { cmd, env, .. } = &body[0] else {
            return Err(Refusal::new("the contract's body is one run primitive"));
        };
        out.outputs
            .insert("echo".into(), rue_hook_sdk::expose(cmd).to_string());
        // The secret the request carried, returned in an output the op
        // declared secret: `execute.run` is one of the four messages of
        // 7.5 that may carry one, in both directions.
        let pw = env
            .iter()
            .find(|(k, _)| k == "PW")
            .ok_or_else(|| Refusal::new("the contract's run carries PW"))?;
        out.outputs
            .insert("secret".into(), rue_hook_sdk::expose(&pw.1).to_string());
        Ok(out)
    }

    fn read_fact(&mut self, _host: &str, shape: &str) -> Answer<Option<String>> {
        Ok(match shape {
            "file:/conformance/present" => Some("present\n".into()),
            // None is "no such file", which is an answer. An empty string
            // would be a file that exists and is empty.
            _ => None,
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
            instance: "conform-1".into(),
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
    fn get_file(&mut self, _h: &str, _i: &str, _rel: &str) -> Answer<String> {
        Ok("1700000000\n".into())
    }
    fn remove_file(&mut self, _h: &str, _i: &str, _r: &str) -> Answer<()> {
        Ok(())
    }
    fn host_lock(&mut self, _h: &str) -> Answer<()> {
        Ok(())
    }
}

impl Probe for World {
    fn observe(&mut self, _host: &str, probe: &str) -> Answer<Observation> {
        match probe {
            "conform-yes" => Ok(Observation::yes("yes")),
            "conform-no" => Ok(Observation::no("no")),
            "conform-unknown" => Ok(Observation::unknown("")),
            "conform-refuse" => Err(Refusal::new(
                "refused as the conformance contract asks, with a reason to read",
            )),
            other => Err(Refusal::new(format!("no probe named {other}"))),
        }
    }
}

impl Approval for World {
    fn authenticators(&mut self) -> Answer<Vec<Authenticator>> {
        Ok(vec![
            Authenticator {
                id: "conform-human".into(),
                human: true,
            },
            Authenticator {
                id: "conform-machine".into(),
                human: false,
            },
        ])
    }

    fn challenge(&mut self, r: &ChallengeRequest) -> Answer<String> {
        Ok(format!(
            "approve {} on {} ({})",
            r.digest,
            r.instance,
            scope_text(&r.scope)
        ))
    }

    fn verify(&mut self, r: &VerifyRequest) -> Answer<Verdict> {
        // The proof is bound to the digest *and* the scope (5.11). Building
        // the expected proof from what the request carries, rather than
        // from anything remembered, is what makes a replay fail.
        let want = format!("{}/{}", r.digest, scope_text(&r.scope));
        Ok(if r.proof == want {
            Verdict {
                verified: true,
                reason: String::new(),
            }
        } else {
            Verdict {
                verified: false,
                reason: "the proof was made for another request or another scope".into(),
            }
        })
    }
}

/// `plan`, `step/<n>`, `ack/<n>` -- the scope as the contract spells it, in
/// a form every language can build the same way.
fn scope_text(scope: &Value) -> String {
    if scope == "plan" {
        return "plan".into();
    }
    if let Some(n) = scope.get("step").and_then(Value::as_u64) {
        return format!("step/{n}");
    }
    if let Some(n) = scope.get("ack").and_then(Value::as_u64) {
        return format!("ack/{n}");
    }
    "unknown".into()
}

impl Secrets for World {
    fn resolve(&mut self, _reference: &str) -> Answer<String> {
        Ok("conformance-resolved-secret".into())
    }
    fn deliver(&mut self, _instance: &str, label: &str, _value: &str) -> Answer<Delivery> {
        // Declining is an answer, not a refusal: the engine offers the
        // secret to the next acceptor.
        Ok(Delivery {
            accepted: label != "unwanted",
            receipt: format!("receipt-{label}"),
        })
    }
}

impl Notify for World {
    fn deliver(&mut self, _l: &str, _s: &str, _b: &str) -> Answer<()> {
        Ok(())
    }
}

impl Scheduler for World {
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
            "conform-present.sh" => Presence::Present,
            "conform-absent.sh" => Presence::Absent,
            // Never guess: the engine reads `false` as "install it again".
            _ => Presence::Unknown,
        })
    }
}

fn hooks() -> Hooks {
    let mut h = Hooks::new();
    h.journal = Some(Box::new(World));
    h.inventory = Some(Box::new(World));
    h.execute = Some(Box::new(World));
    h.probe = Some(Box::new(World));
    h.approval = Some(Box::new(World));
    h.secrets = Some(Box::new(World));
    h.notify = Some(Box::new(World));
    h.scheduler = Some(Box::new(World));
    h.filesystem = true;
    h.stdin_preamble = true;
    h
}

/// The provocation this request asks for, if any.
fn provocation(frame: &Value) -> Option<&str> {
    if frame.get("kind").and_then(Value::as_str) != Some("probe") {
        return None;
    }
    match frame.get("probe").and_then(Value::as_str) {
        Some(p @ ("conform-missing-field" | "conform-no-ok" | "conform-silent")) => Some(p),
        _ => None,
    }
}

fn main() -> std::io::Result<()> {
    let name = std::env::args().nth(1).unwrap_or_else(|| "conform".into());
    let mut hooks = hooks();
    let stdin = std::io::stdin();
    let mut r = BufReader::new(stdin.lock());
    let mut w = std::io::stdout();

    // The handshake: the register frame first, then the acknowledgement.
    let reg: Registration = rue_hook_sdk::registration(&name, &hooks);
    debug_assert_eq!(reg.protocol, HOOK_PROTOCOL);
    write_frame(&mut w, &json!({ "register": reg }))?;
    match read_frame(&mut r)? {
        Some(ack) if ack.pointer("/register/ok") == Some(&json!(true)) => {}
        other => {
            eprintln!("conform-hook: registration was not acknowledged: {other:?}");
            std::process::exit(1);
        }
    }

    while let Some(frame) = read_frame(&mut r)? {
        if frame.is_null() || frame.get("kind").is_none() {
            continue;
        }
        let id = frame.get("id").cloned().unwrap_or(Value::Null);
        match provocation(&frame) {
            // Deliberately malformed, and deliberately not through the SDK:
            // `Hooks::answer` builds replies from the op's row and cannot
            // express either of these.
            Some("conform-missing-field") => write_frame(&mut w, &json!({ "id": id, "ok": true }))?,
            Some("conform-no-ok") => write_frame(&mut w, &json!({ "id": id }))?,
            // Say nothing at all: the engine reads that as Silent.
            Some("conform-silent") => {
                let _ = w.flush();
            }
            _ => {
                let reply = hooks.answer(&frame);
                write_frame(&mut w, &reply)?;
            }
        }
    }
    Ok(())
}
