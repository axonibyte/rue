//! The request digest and its scoped forms, docs/ROADMAP.md section 5.11.
//! Rue supplies a digest, not a challenge: the approval binding renders
//! whatever human-facing challenge it likes over the digest and verifies
//! proofs against it. The nonce and the time are the caller's: core has
//! neither randomness nor a clock.

use serde_json::Value;

use crate::canon::{message, Canon, Encoder};
use crate::journal::{Hash, Scope};
use crate::json::canonical::{encode, CanonicalError};
use crate::model::{Duration, Instant};

pub const DOMAIN: &str = "rue-request";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// 32 bytes from the engine's CSPRNG.
    pub nonce: [u8; 32],
    pub plan_id: String,
    pub instance: String,
    pub owner_host: String,
    pub params_hash: Hash,
    pub host_contract_hash: Hash,
    pub wane: Option<Duration>,
    pub requested_at: Instant,
    pub gate_hash: Hash,
    pub plan_content_hash: Hash,
}

impl Canon for Request {
    fn canon(&self, e: &mut Encoder) {
        e.record(10);
        e.octets(&self.nonce);
        e.str(&self.plan_id);
        e.str(&self.instance);
        e.str(&self.owner_host);
        self.params_hash.canon(e);
        self.host_contract_hash.canon(e);
        match self.wane {
            None => e.none(),
            Some(d) => e.some(&d.seconds),
        }
        e.u64(self.requested_at.unix_s);
        self.gate_hash.canon(e);
        self.plan_content_hash.canon(e);
    }
}

/// `H(canonical(nonce, plan_id, instance, owner_host, params_hash,
/// host_contract_hash, wane, requested_at, gate_hash, plan_content_hash))`,
/// domain-separated `rue-request`.
pub fn request_digest(r: &Request) -> Hash {
    Hash::sha256(&message(DOMAIN, r))
}

/// The digest a proof binds to in a scope: the request digest itself for
/// plan entry; for a step gate or an acknowledgement, a digest over the
/// scope's name, the request digest and the step index, so a proof for one
/// step or scope verifies for no other.
pub fn scoped_digest(request: &Hash, scope: Scope) -> Hash {
    struct Scoped<'a>(&'a Hash, &'a str, u32);
    impl Canon for Scoped<'_> {
        fn canon(&self, e: &mut Encoder) {
            e.record(3);
            e.str(self.1);
            self.0.canon(e);
            e.u64(u64::from(self.2));
        }
    }
    match scope {
        Scope::Plan => *request,
        Scope::Step(n) => Hash::sha256(&message(DOMAIN, &Scoped(request, "step", n))),
        Scope::Ack(n) => Hash::sha256(&message(DOMAIN, &Scoped(request, "ack", n))),
    }
}

/// SHA-256 of a value's canonical JSON, for `params_hash`, `gate_hash` and
/// `plan_content_hash`.
pub fn hash_json(v: &Value) -> Result<Hash, CanonicalError> {
    Ok(Hash::sha256(&encode(v)?))
}
