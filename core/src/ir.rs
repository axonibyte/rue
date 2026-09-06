//! The plan IR: what `check` consumes -- a site, a requester and one concrete
//! per-host plan -- as one JSON document (docs/TESTING.md, "The plan IR").
//! The Phase 0 prototype emits it; this crate reads it; Phase 2's front end
//! will produce it.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Plan, Site};

/// The IR version this crate reads. Any other is refused.
pub const IR_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanIr {
    pub ir_version: u32,
    pub requester: String,
    pub site: Site,
    pub plan: Plan,
}

#[derive(Debug)]
pub enum IrError {
    /// Not JSON, or not this shape.
    Json(serde_json::Error),
    /// A version this crate does not read.
    Version { found: u32 },
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrError::Json(e) => write!(f, "plan IR: {e}"),
            IrError::Version { found } => {
                write!(
                    f,
                    "plan IR version {found} is not version {IR_VERSION}, the one this build reads"
                )
            }
        }
    }
}

impl std::error::Error for IrError {}

/// Parse a plan IR document. The version is checked before the shape, so a
/// document from another version says so rather than failing on a field.
pub fn parse(bytes: &[u8]) -> Result<PlanIr, IrError> {
    #[derive(Deserialize)]
    struct Head {
        ir_version: u32,
    }
    let head: Head = serde_json::from_slice(bytes).map_err(IrError::Json)?;
    if head.ir_version != IR_VERSION {
        return Err(IrError::Version {
            found: head.ir_version,
        });
    }
    serde_json::from_slice(bytes).map_err(IrError::Json)
}
