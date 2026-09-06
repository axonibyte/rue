//! Canonical JSON: the byte format specified in docs/TESTING.md and first
//! implemented by `Rue.Proto.Json.Canonical` in the prototype.
//!
//! - UTF-8; object keys sorted by code point; two-space indent; `"key": value`;
//!   every array or object element on its own line; `[]` and `{}` for empty
//!   containers; one trailing LF; no trailing whitespace.
//! - Numbers are integers only. A non-integer is refused so no golden can
//!   depend on a float format.
//! - Strings escape `"`, `\` and controls below U+0020 (`\n \r \t \b \f` by
//!   name, else `\u00xx` in lowercase hex); everything else is raw UTF-8.
//!
//! `serde_json`'s pretty printer over a `Value` (whose map is a `BTreeMap`,
//! so keys are already in code-point order) produces exactly these bytes
//! once a newline is appended; the tests in `tests/canonical.rs` hold it to
//! them byte for byte, so a change in the crate's formatter fails there
//! first.

use std::fmt;

use serde_json::Value;

/// Why a value has no canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// A number that is not an integer, at the given path (`$.a[2].b`).
    NonInteger { path: String },
    /// The serializer refused (it should not, for a `Value`; kept as an error
    /// rather than a panic so the crate never aborts on data).
    Serialize(String),
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalError::NonInteger { path } => {
                write!(f, "canonical JSON admits integers only; {path} is not one")
            }
            CanonicalError::Serialize(e) => write!(f, "canonical JSON: {e}"),
        }
    }
}

impl std::error::Error for CanonicalError {}

/// Encode a value canonically, or say why it cannot be.
pub fn encode(value: &Value) -> Result<Vec<u8>, CanonicalError> {
    check_integers(value, &mut String::from("$"))?;
    let mut out =
        serde_json::to_vec_pretty(value).map_err(|e| CanonicalError::Serialize(e.to_string()))?;
    out.push(b'\n');
    Ok(out)
}

fn check_integers(value: &Value, path: &mut String) -> Result<(), CanonicalError> {
    match value {
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                Ok(())
            } else {
                Err(CanonicalError::NonInteger { path: path.clone() })
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                let len = path.len();
                path.push_str(&format!("[{i}]"));
                check_integers(item, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        Value::Object(map) => {
            for (k, v) in map {
                let len = path.len();
                path.push('.');
                path.push_str(k);
                check_integers(v, path)?;
                path.truncate(len);
            }
            Ok(())
        }
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
    }
}
