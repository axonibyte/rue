//! One constructor per op of docs/ROADMAP.md 7.5.
//!
//! Each returns the request's body without its `id`, which whoever owns the
//! connection assigns. The constructors take secret-bearing values only for
//! the messages [`crate::Op::may_carry_secret`] names; no other message has
//! a place to put one, which is what makes the engine's R0305 guard a check
//! on a mistake rather than the only thing standing between a secret and
//! the wire.

use rue_core::journal::Entry;
use serde_json::{json, Value};

use crate::body::RPrim;

/// The two fields every request opens with.
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

pub fn execute_read_fact(host: &str, shape: &str) -> Value {
    let mut v = execute_op("read_fact", host, None);
    v["shape"] = json!(shape);
    v
}

pub fn execute_put_file(host: &str, instance: &str, rel: &str, content: &str, mode: u32) -> Value {
    let mut v = execute_op("put_file", host, Some(instance));
    v["rel"] = json!(rel);
    v["content"] = json!(content);
    v["mode"] = json!(mode);
    v
}

pub fn execute_replace_file(host: &str, instance: &str, rel: &str, content: &str) -> Value {
    let mut v = execute_op("replace_file", host, Some(instance));
    v["rel"] = json!(rel);
    v["content"] = json!(content);
    v
}

pub fn execute_get_file(host: &str, instance: &str, rel: &str) -> Value {
    let mut v = execute_op("get_file", host, Some(instance));
    v["rel"] = json!(rel);
    v
}

pub fn execute_remove_file(host: &str, instance: &str, rel: &str) -> Value {
    let mut v = execute_op("remove_file", host, Some(instance));
    v["rel"] = json!(rel);
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

#[cfg(test)]
mod tests {
    use crate::op::{Op, OPS};
    use serde_json::Value;

    /// Every constructor names a kind and an op the table has a row for,
    /// and puts exactly the fields that row declares. A field added to a
    /// frame without a row is what this catches.
    #[test]
    fn every_constructor_matches_its_row() {
        let scope = serde_json::json!({"plan": true});
        let mut built: Vec<Value> = vec![
            super::inventory_list(),
            super::execute_run("h", "i", &[]),
            super::probe_observe("h", "p"),
            super::approval_authenticators(),
            super::approval_challenge("i", "d", scope.clone(), Value::Null),
            super::approval_verify("i", "d", scope, "a", "p"),
            super::secrets_resolve("r"),
            super::secrets_deliver("i", "l", "v"),
            super::notify_deliver("waiting", "s", "b"),
            super::execute_read_fact("h", "file:/etc/rc.conf"),
            super::execute_put_file("h", "i", "markers/1", "owned", 0o640),
            super::execute_replace_file("h", "i", "deadline", "9"),
            super::execute_get_file("h", "i", "deadline"),
            super::execute_remove_file("h", "i", "markers/1"),
        ];
        for op in ["clock", "bootstrap_state", "host_lock", "instance_dir_list"] {
            built.push(super::execute_op(op, "h", None));
        }
        for op in ["instance_dir_create", "instance_dir_remove"] {
            built.push(super::execute_op(op, "h", Some("i")));
        }
        for op in ["install", "arm", "rearm", "disarm", "present"] {
            built.push(super::scheduler_op(op, "h", "a.sh", None));
        }
        for v in &built {
            let kind = v["kind"].as_str().unwrap();
            let op = v["op"].as_str().unwrap();
            let row = Op::find(kind, op).unwrap_or_else(|| panic!("{kind}.{op} has no row"));
            for (k, _) in v.as_object().unwrap() {
                if k == "kind" || k == "op" {
                    continue;
                }
                assert!(
                    row.request.contains(&k.as_str()),
                    "{kind}.{op} carries `{k}`, which its row does not declare"
                );
            }
        }
    }

    /// `execute_op` and `scheduler_op` take an op name as a string, so the
    /// rows they can reach are the ones nothing else builds. Together with
    /// the test above, every row of the table is constructible.
    #[test]
    fn every_row_is_reachable_by_some_constructor() {
        let built: Vec<(&str, &str)> = vec![
            ("journal", "append"),
            ("inventory", "list"),
            ("execute", "run"),
            ("execute", "read_fact"),
            ("execute", "bootstrap_state"),
            ("execute", "clock"),
            ("execute", "instance_dir_create"),
            ("execute", "instance_dir_remove"),
            ("execute", "instance_dir_list"),
            ("execute", "put_file"),
            ("execute", "replace_file"),
            ("execute", "get_file"),
            ("execute", "remove_file"),
            ("execute", "host_lock"),
            ("probe", "observe"),
            ("approval", "authenticators"),
            ("approval", "challenge"),
            ("approval", "verify"),
            ("secrets", "resolve"),
            ("secrets", "deliver"),
            ("notify", "deliver"),
            ("scheduler", "install"),
            ("scheduler", "arm"),
            ("scheduler", "rearm"),
            ("scheduler", "disarm"),
            ("scheduler", "present"),
        ];
        let rows: Vec<(&str, &str)> = OPS.iter().map(|o| (o.kind, o.op)).collect();
        assert_eq!(built, rows, "a row is unreachable or unlisted");
    }
}
