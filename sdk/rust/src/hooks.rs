//! [`Hooks`]: the kinds a hook actually implements, and the dispatch from a
//! request frame to the handler for it.
//!
//! The registration frame is built from what is filled in here, so a hook
//! cannot register for a kind it does not serve -- the failure mode that
//! produces is a plan that binds to it and then refuses at the first step,
//! which is a long way from where the mistake was made.

use rue_hook_proto::{Observation, Op, Output, RPrim};
use serde_json::{json, Value};

use crate::{
    ok_frame, refusal_frame, Answer, Approval, ChallengeRequest, Execute, Inventory, Journal,
    Notify, Probe, Refusal, Scheduler, Secrets, VerifyRequest,
};

/// Whether a scheduler entry is there. `Unknown` is never read as absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Present,
    Absent,
    Unknown,
}

impl Presence {
    fn to_value(self) -> Value {
        match self {
            Presence::Present => json!(true),
            Presence::Absent => json!(false),
            Presence::Unknown => json!("unknown"),
        }
    }
}

/// What this hook serves. Fill in the kinds you implement and leave the
/// rest; [`crate::registration`] reads the kinds off it.
#[derive(Default)]
pub struct Hooks {
    pub journal: Option<Box<dyn Journal>>,
    pub inventory: Option<Box<dyn Inventory>>,
    pub execute: Option<Box<dyn Execute>>,
    pub probe: Option<Box<dyn Probe>>,
    pub approval: Option<Box<dyn Approval>>,
    pub secrets: Option<Box<dyn Secrets>>,
    pub notify: Option<Box<dyn Notify>>,
    pub scheduler: Option<Box<dyn Scheduler>>,
    /// This hook serves the instance-directory ops (7.7). Declaring it
    /// without implementing them is what R0408 catches at run time, after
    /// a plan has already been admitted, so declare it only when the
    /// `Execute` above answers all of them.
    pub filesystem: bool,
    /// `env:` and `stdin:` reach the command through a preamble on stdin,
    /// never on a command line.
    pub stdin_preamble: bool,
}

impl Hooks {
    pub fn new() -> Hooks {
        Hooks::default()
    }

    /// The kinds served, in the order docs/ROADMAP.md 7.5 lists them.
    pub fn kinds(&self) -> Vec<String> {
        let mut k = Vec::new();
        if self.journal.is_some() {
            k.push("journal".to_string());
        }
        if self.inventory.is_some() {
            k.push("inventory".to_string());
        }
        if self.execute.is_some() {
            k.push("execute".to_string());
        }
        if self.probe.is_some() {
            k.push("probe".to_string());
        }
        if self.approval.is_some() {
            k.push("approval".to_string());
        }
        if self.secrets.is_some() {
            k.push("secrets".to_string());
        }
        if self.notify.is_some() {
            k.push("notify".to_string());
        }
        if self.scheduler.is_some() {
            k.push("scheduler".to_string());
        }
        k
    }

    /// Answer one request frame. The reply always carries the request's id,
    /// including for a request this hook has no row or no handler for: a
    /// reply the engine can match is the difference between a refusal and
    /// a silence.
    pub fn answer(&mut self, request: &Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let kind = request.get("kind").and_then(Value::as_str).unwrap_or("");
        let op_name = request.get("op").and_then(Value::as_str).unwrap_or("");
        let Some(op) = Op::find(kind, op_name) else {
            return refusal_frame(
                id,
                &Refusal::new(format!("{kind}.{op_name} is not an op of this protocol")),
            );
        };
        match self.dispatch(op, request) {
            Ok(fields) => ok_frame(id, op, fields),
            Err(why) => refusal_frame(id, &why),
        }
    }

    fn dispatch(&mut self, op: &Op, r: &Value) -> Answer<Value> {
        let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        match (op.kind, op.op) {
            ("journal", "append") => {
                let h = self.journal.as_mut().ok_or_else(|| unserved(op))?;
                let entry = r
                    .get("entry")
                    .ok_or_else(|| Refusal::new("journal.append without an entry"))?;
                h.append(entry).map(|()| json!({}))
            }
            ("inventory", "list") => {
                let h = self.inventory.as_mut().ok_or_else(|| unserved(op))?;
                let hosts = h.list()?;
                Ok(json!({ "hosts": to_value(&hosts)? }))
            }
            ("execute", "run") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                let body: Vec<RPrim> =
                    from_value(r.get("body").cloned().unwrap_or(json!([])), "body")?;
                let out: Output = h.run(&s("host"), &s("instance"), &body)?;
                Ok(json!({ "output": to_value(&out)? }))
            }
            ("execute", "read_fact") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                Ok(match h.read_fact(&s("host"), &s("shape"))? {
                    // No such file is an absent field, not an empty one.
                    None => json!({}),
                    Some(c) => json!({ "content": c }),
                })
            }
            ("execute", "bootstrap_state") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                let st = h.bootstrap_state(&s("host"))?;
                Ok(json!({ "state": to_value(&st)? }))
            }
            ("execute", "clock") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                Ok(json!({ "epoch_s": h.clock(&s("host"))? }))
            }
            ("execute", "instance_dir_create") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                h.instance_dir_create(&s("host"), &s("instance"))
                    .map(|()| json!({}))
            }
            ("execute", "instance_dir_remove") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                h.instance_dir_remove(&s("host"), &s("instance"))
                    .map(|()| json!({}))
            }
            ("execute", "instance_dir_list") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                let dirs = h.instance_dir_list(&s("host"))?;
                Ok(json!({ "dirs": to_value(&dirs)? }))
            }
            ("execute", "put_file") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                let mode = r.get("mode").and_then(Value::as_u64).unwrap_or(0) as u32;
                h.put_file(&s("host"), &s("instance"), &s("rel"), &s("content"), mode)
                    .map(|()| json!({}))
            }
            ("execute", "replace_file") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                h.replace_file(&s("host"), &s("instance"), &s("rel"), &s("content"))
                    .map(|()| json!({}))
            }
            ("execute", "get_file") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                let c = h.get_file(&s("host"), &s("instance"), &s("rel"))?;
                Ok(json!({ "content": c }))
            }
            ("execute", "remove_file") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                h.remove_file(&s("host"), &s("instance"), &s("rel"))
                    .map(|()| json!({}))
            }
            ("execute", "host_lock") => {
                let h = self.execute.as_mut().ok_or_else(|| unserved(op))?;
                h.host_lock(&s("host")).map(|()| json!({}))
            }
            ("probe", "observe") => {
                let h = self.probe.as_mut().ok_or_else(|| unserved(op))?;
                let fact: Observation = h.observe(&s("host"), &s("probe"))?;
                Ok(json!({ "fact": to_value(&fact)? }))
            }
            ("approval", "authenticators") => {
                let h = self.approval.as_mut().ok_or_else(|| unserved(op))?;
                let auths: Vec<Value> = h
                    .authenticators()?
                    .into_iter()
                    .map(|a| json!({ "id": a.id, "human": a.human }))
                    .collect();
                Ok(json!({ "authenticators": auths }))
            }
            ("approval", "challenge") => {
                let h = self.approval.as_mut().ok_or_else(|| unserved(op))?;
                let c = h.challenge(&ChallengeRequest {
                    instance: s("instance"),
                    digest: s("digest"),
                    scope: r.get("scope").cloned().unwrap_or(Value::Null),
                    context: r.get("context").cloned().unwrap_or(Value::Null),
                })?;
                Ok(json!({ "challenge": c }))
            }
            ("approval", "verify") => {
                let h = self.approval.as_mut().ok_or_else(|| unserved(op))?;
                let v = h.verify(&VerifyRequest {
                    instance: s("instance"),
                    digest: s("digest"),
                    scope: r.get("scope").cloned().unwrap_or(Value::Null),
                    authenticator: s("authenticator"),
                    proof: s("proof"),
                })?;
                Ok(json!({ "verified": v.verified, "reason": v.reason }))
            }
            ("secrets", "resolve") => {
                let h = self.secrets.as_mut().ok_or_else(|| unserved(op))?;
                Ok(json!({ "value": h.resolve(&s("ref"))? }))
            }
            ("secrets", "deliver") => {
                let h = self.secrets.as_mut().ok_or_else(|| unserved(op))?;
                let d = h.deliver(&s("instance"), &s("label"), &s("value"))?;
                Ok(json!({ "accepted": d.accepted, "receipt": d.receipt }))
            }
            ("notify", "deliver") => {
                let h = self.notify.as_mut().ok_or_else(|| unserved(op))?;
                h.deliver(&s("level"), &s("subject"), &s("body"))
                    .map(|()| json!({}))
            }
            ("scheduler", op_name) => {
                let deadline = r.get("deadline").and_then(Value::as_u64);
                let h = self.scheduler.as_mut().ok_or_else(|| unserved(op))?;
                let (host, artifact) = (s("host"), s("artifact"));
                match op_name {
                    "install" => h.install(&host, &artifact).map(|()| json!({})),
                    "arm" => h.arm(&host, &artifact, deadline).map(|()| json!({})),
                    "rearm" => h.rearm(&host, &artifact, deadline).map(|()| json!({})),
                    "disarm" => h.disarm(&host, &artifact).map(|()| json!({})),
                    "present" => Ok(json!({ "present": h.present(&host, &artifact)?.to_value() })),
                    other => Err(Refusal::new(format!("scheduler.{other} is unhandled"))),
                }
            }
            (kind, name) => Err(Refusal::new(format!("{kind}.{name} is unhandled"))),
        }
    }
}

fn unserved(op: &Op) -> Refusal {
    Refusal::unserved(op.kind, op.op)
}

fn to_value<T: serde::Serialize>(v: &T) -> Answer<Value> {
    serde_json::to_value(v).map_err(|e| Refusal::new(format!("cannot encode the answer: {e}")))
}

fn from_value<T: serde::de::DeserializeOwned>(v: Value, what: &str) -> Answer<T> {
    serde_json::from_value(v).map_err(|e| Refusal::new(format!("cannot read `{what}`: {e}")))
}
