//! The engine's view of a host: core's record (what a check reads) plus
//! what an executor needs to reach it and what an arming needs to know
//! about it. The inventory binding produces these (Appendix C); the check's
//! `Site` is derived from them, never the other way around.

use std::collections::BTreeMap;

use rue_core::model::HostRecord;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    pub record: HostRecord,
    /// What the transport dials.
    pub address: String,
    /// The scheduler binding's name on this host (`cron`, `task-scheduler`,
    /// `launchd`), when one is present.
    pub scheduler: Option<String>,
    /// The target's `rue_root`; `None` is the OS family's default.
    pub rue_root: Option<String>,
    /// Contract facts beyond name and os (roles, and whatever the record
    /// carried) that a `host.<field>` reference or a clause may read.
    #[serde(default)]
    pub facts: BTreeMap<String, String>,
}

impl Host {
    pub fn name(&self) -> &str {
        &self.record.name
    }

    /// A `host.<field>` reference: name, os and address are fields of the
    /// record proper; anything else is a contract fact.
    pub fn field(&self, field: &str) -> Option<String> {
        match field {
            "name" => Some(self.record.name.clone()),
            "os" => Some(self.record.os.clone()),
            "address" => Some(self.address.clone()),
            other => self.facts.get(other).cloned(),
        }
    }
}
