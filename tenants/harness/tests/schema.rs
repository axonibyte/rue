//! Tier 3: docs/verdict-schema.json against the verdicts the crates produce.
//!
//! A minimal validator supporting exactly the keywords the schema uses --
//! type (including a list with "null"), properties, required,
//! additionalProperties (false or a schema), items, enum, anyOf -- and
//! refusing any other keyword, so an unsupported construct fails rather than
//! passes. Every verdict validates; every property path the schema declares
//! is produced by at least one verdict (a field nothing produces is a claim
//! the verdict does not make); verdict_version agrees.

use std::collections::BTreeSet;
use std::fs;

use rue_core::verdict::VERDICT_VERSION;
use rue_tenants::artifacts;
use rue_tenants::golden::repo_root;
use serde_json::Value;

const SUPPORTED: &[&str] = &[
    "$schema",
    "$id",
    "title",
    "description",
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "anyOf",
];

fn schema_and_verdicts() -> (Value, Vec<(String, Value)>) {
    let root = repo_root().unwrap();
    let schema: Value =
        serde_json::from_slice(&fs::read(root.join("docs/verdict-schema.json")).unwrap()).unwrap();
    let verdicts: Vec<(String, Value)> = artifacts(&root)
        .into_iter()
        .filter(|a| a.path.ends_with("verdict.json"))
        .map(|a| {
            (
                a.path.clone(),
                serde_json::from_slice(a.bytes.as_ref().unwrap()).unwrap(),
            )
        })
        .collect();
    (schema, verdicts)
}

fn subschemas(o: &serde_json::Map<String, Value>) -> Vec<&Value> {
    let mut v = Vec::new();
    if let Some(Value::Object(ps)) = o.get("properties") {
        v.extend(ps.values());
    }
    if let Some(a @ Value::Object(_)) = o.get("additionalProperties") {
        v.push(a);
    }
    if let Some(i) = o.get("items") {
        v.push(i);
    }
    if let Some(Value::Array(xs)) = o.get("anyOf") {
        v.extend(xs.iter());
    }
    v
}

fn unsupported(v: &Value) -> Vec<String> {
    match v {
        Value::Object(o) => {
            let mut out: Vec<String> = o
                .keys()
                .filter(|k| !SUPPORTED.contains(&k.as_str()))
                .cloned()
                .collect();
            for s in subschemas(o) {
                out.extend(unsupported(s));
            }
            out
        }
        _ => Vec::new(),
    }
}

fn matches_type(v: &Value, t: &str) -> bool {
    match (t, v) {
        ("object", Value::Object(_))
        | ("array", Value::Array(_))
        | ("string", Value::String(_))
        | ("boolean", Value::Bool(_))
        | ("null", Value::Null)
        | ("number", Value::Number(_)) => true,
        ("integer", Value::Number(n)) => n.is_i64() || n.is_u64(),
        _ => false,
    }
}

fn summary(v: &Value) -> String {
    match v {
        Value::Object(_) => "an object".into(),
        Value::Array(_) => "an array".into(),
        other => other.to_string(),
    }
}

fn validate(schema: &Value, v: &Value) -> Vec<String> {
    let Value::Object(o) = schema else {
        return vec!["schema is not an object".into()];
    };
    let mut errs = Vec::new();
    if let Some(t) = o.get("type") {
        let allowed: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(xs) => xs.iter().filter_map(|x| x.as_str()).collect(),
            _ => Vec::new(),
        };
        if !allowed.iter().any(|t| matches_type(v, t)) {
            errs.push(format!("type {allowed:?} does not admit {}", summary(v)));
        }
    }
    if let Some(Value::Array(xs)) = o.get("enum") {
        if !xs.contains(v) {
            errs.push(format!("value {} not in enum", summary(v)));
        }
    }
    if let Value::Object(obj) = v {
        let empty = serde_json::Map::new();
        let props = match o.get("properties") {
            Some(Value::Object(ps)) => ps,
            _ => &empty,
        };
        if let Some(Value::Array(req)) = o.get("required") {
            let missing: Vec<&str> = req
                .iter()
                .filter_map(|r| r.as_str())
                .filter(|r| !obj.contains_key(*r))
                .collect();
            if !missing.is_empty() {
                errs.push(format!("missing required {missing:?}"));
            }
        }
        let extra: Vec<&String> = obj.keys().filter(|k| !props.contains_key(*k)).collect();
        match o.get("additionalProperties") {
            Some(Value::Bool(false)) if !extra.is_empty() => {
                errs.push(format!("unexpected properties {extra:?}"))
            }
            Some(s @ Value::Object(_)) => {
                for k in &extra {
                    errs.extend(
                        validate(s, &obj[*k])
                            .into_iter()
                            .map(|e| format!("{k}: {e}")),
                    );
                }
            }
            _ => {}
        }
        for (k, x) in obj {
            if let Some(s) = props.get(k) {
                errs.extend(validate(s, x).into_iter().map(|e| format!("{k}: {e}")));
            }
        }
    }
    if let (Some(s), Value::Array(xs)) = (o.get("items"), v) {
        for (i, x) in xs.iter().enumerate() {
            errs.extend(validate(s, x).into_iter().map(|e| format!("[{i}]: {e}")));
        }
    }
    if let Some(Value::Array(alts)) = o.get("anyOf") {
        if !alts.iter().any(|alt| validate(alt, v).is_empty()) {
            errs.push(format!("no anyOf alternative admits {}", summary(v)));
        }
    }
    errs
}

/// Every property path the schema declares, arrays as "[]", map-valued
/// objects (additionalProperties as a schema) as "{}".
fn schema_paths(prefix: &[String], v: &Value, out: &mut BTreeSet<String>) {
    let Value::Object(o) = v else { return };
    if !prefix.is_empty() {
        out.insert(prefix.join("."));
    }
    let extend = |p: &[String], k: &str| -> Vec<String> {
        let mut v = p.to_vec();
        v.push(k.to_string());
        v
    };
    if let Some(Value::Object(ps)) = o.get("properties") {
        for (k, s) in ps {
            schema_paths(&extend(prefix, k), s, out);
        }
    }
    if let Some(s @ Value::Object(_)) = o.get("additionalProperties") {
        schema_paths(&extend(prefix, "{}"), s, out);
    }
    if let Some(s) = o.get("items") {
        schema_paths(&extend(prefix, "[]"), s, out);
    }
    if let Some(Value::Array(xs)) = o.get("anyOf") {
        for s in xs {
            schema_paths(prefix, s, out);
        }
    }
}

/// Every path a value populates with a non-null value. The two map-valued
/// fields (hosts_touched, drift_policy) collapse their keys to "{}".
fn value_paths(prefix: &[String], v: &Value, out: &mut BTreeSet<String>) {
    if !prefix.is_empty() && !v.is_null() {
        out.insert(prefix.join("."));
    }
    let map_valued = matches!(
        prefix.last().map(String::as_str),
        Some("hosts_touched") | Some("drift_policy")
    );
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                let mut p = prefix.to_vec();
                p.push(if map_valued {
                    "{}".to_string()
                } else {
                    k.clone()
                });
                value_paths(&p, x, out);
            }
        }
        Value::Array(xs) => {
            let mut p = prefix.to_vec();
            p.push("[]".to_string());
            for x in xs {
                value_paths(&p, x, out);
            }
        }
        _ => {}
    }
}

#[test]
fn the_schema_uses_only_supported_keywords() {
    let (schema, _) = schema_and_verdicts();
    assert_eq!(unsupported(&schema), Vec::<String>::new());
}

#[test]
fn every_verdict_validates() {
    let (schema, verdicts) = schema_and_verdicts();
    assert!(!verdicts.is_empty());
    for (path, v) in &verdicts {
        let errs = validate(&schema, v);
        assert!(errs.is_empty(), "{path}:\n  {}", errs.join("\n  "));
    }
}

#[test]
fn every_schema_property_path_is_produced_by_at_least_one_verdict() {
    let (schema, verdicts) = schema_and_verdicts();
    let mut declared = BTreeSet::new();
    schema_paths(&[], &schema, &mut declared);
    let mut produced = BTreeSet::new();
    for (_, v) in &verdicts {
        value_paths(&[], v, &mut produced);
    }
    let missing: Vec<&String> = declared.difference(&produced).collect();
    assert_eq!(missing, Vec::<&String>::new());
}

#[test]
fn verdict_version_agrees() {
    let (_, verdicts) = schema_and_verdicts();
    for (path, v) in &verdicts {
        assert_eq!(
            v["verdict_version"],
            serde_json::json!(VERDICT_VERSION),
            "{path}"
        );
    }
}
