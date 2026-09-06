//! The cross-plan ledger, docs/ROADMAP.md section 5.12, as a pure value: per
//! host, the umbras of active and pending instances and the exclusivity
//! classes they hold. A new instance overlapping a reserved umbra is refused
//! at request (R0203), regardless of exclusivity class, before any proof is
//! collected; a second instance in a held class is refused with R0101 (exit
//! 75). A request dry-run reserves nothing and is never blocked by nothing.

use crate::interference::Fact;
use crate::model::Host;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub id: String,
    pub host: Host,
    pub umbra: Vec<Fact>,
    pub exclusivity: Option<String>,
    pub rehearsal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerCode {
    /// Exclusivity held by another instance (exit 75).
    R0101,
    /// Cross-plan umbra overlap with an active or pending instance.
    R0203,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ledger {
    held: Vec<Instance>,
}

/// Facts touch on equal shapes; two regions must also share an anchor.
fn touches(f: &Fact, g: &Fact) -> bool {
    if f.shape != g.shape {
        return false;
    }
    match (&f.anchor, &g.anchor) {
        (Some(x), Some(y)) => x == y,
        _ => true,
    }
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger::default()
    }

    /// Reserve at request. A rehearsal is admitted and records nothing.
    pub fn request(&self, inst: Instance) -> Result<Ledger, (LedgerCode, String)> {
        if inst.rehearsal {
            return Ok(self.clone());
        }
        let same_host: Vec<&Instance> = self.held.iter().filter(|h| h.host == inst.host).collect();
        if let Some(h) = same_host
            .iter()
            .find(|h| matches!((&h.exclusivity, &inst.exclusivity), (Some(a), Some(b)) if a == b))
        {
            return Err((
                LedgerCode::R0101,
                format!("exclusivity class held by {}", h.id),
            ));
        }
        if let Some(h) = same_host.iter().find(|h| {
            inst.umbra
                .iter()
                .any(|f| h.umbra.iter().any(|g| touches(f, g)))
        }) {
            return Err((LedgerCode::R0203, format!("umbra overlaps {}", h.id)));
        }
        let mut held = vec![inst];
        held.extend(self.held.iter().cloned());
        Ok(Ledger { held })
    }

    /// Release on cancel, lapse, close or commit.
    pub fn release(&self, id: &str) -> Ledger {
        Ledger {
            held: self.held.iter().filter(|h| h.id != id).cloned().collect(),
        }
    }

    pub fn holdings(&self) -> &[Instance] {
        &self.held
    }
}
