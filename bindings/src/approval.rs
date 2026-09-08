//! `always()`, the approval binding of docs/ROADMAP.md 7.3 that approves
//! everything.
//!
//! It exists for daemon dry-run mode (7.9): a rehearsal evaluates and
//! journals its gates, reserves nothing and calls no executor, and a site
//! whose real approval binding is a hook cannot be asked for a human's
//! proof in a rehearsal. `rued` refuses to build it outside `--dry-run`,
//! so no live daemon can be talked into opening a gate with it.

use rue_core::model::Authenticator;
use rue_engine::executor::ExecError;
use rue_engine::gates::{Approval, ProofRequest, Verified};

#[derive(Debug, Default)]
pub struct Always;

impl Approval for Always {
    fn name(&self) -> &str {
        "always"
    }

    /// It publishes none: there is no one to ask.
    fn authenticators(&mut self) -> Result<Vec<Authenticator>, ExecError> {
        Ok(Vec::new())
    }

    fn challenge(&mut self, _r: &ProofRequest) -> Result<String, ExecError> {
        Ok("always(): no proof is asked for".into())
    }

    fn verify(&mut self, _r: &ProofRequest) -> Result<Verified, ExecError> {
        Ok(Verified {
            verified: true,
            reason: "always()".into(),
        })
    }

    fn approves_everything(&self) -> bool {
        true
    }
}
