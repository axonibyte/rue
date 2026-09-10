//! The journal model, docs/ROADMAP.md section 5.10: the entry type, its
//! canonical bytes, the hash chain, and the signature slot. No secret value
//! can be typed into an entry -- `secret_labels` carries labels only -- and
//! nothing from the body model is reachable from here. Signing (Ed25519
//! SSHSIG over the canonical entry, namespace `rue-journal`) fills the slot in
//! a later unit; the chain verifies without it.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::canon::{message, Canon, Encoder};
use crate::model::Instant;

pub const DOMAIN: &str = "rue-journal";

/// A SHA-256 digest, hex in JSON and text.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    /// The genesis entry's `prev_hash`.
    pub const ZERO: Hash = Hash([0; 32]);

    pub fn sha256(bytes: &[u8]) -> Hash {
        Hash(Sha256::digest(bytes).into())
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn from_hex(s: &str) -> Option<Hash> {
        if s.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16)?;
            let lo = (chunk[1] as char).to_digit(16)?;
            out[i] = (hi * 16 + lo) as u8;
        }
        Some(Hash(out))
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", self.to_hex())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for Hash {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Hash, D::Error> {
        let s = String::deserialize(d)?;
        Hash::from_hex(&s)
            .ok_or_else(|| serde::de::Error::custom("a hash is 64 lowercase hex digits"))
    }
}

impl Canon for Hash {
    fn canon(&self, e: &mut Encoder) {
        e.octets(&self.0);
    }
}

/// The signature slot: an SSHSIG over the canonical entry in the named
/// namespace. Bytes are hex in JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sig {
    pub namespace: String,
    pub bytes_hex: String,
}

/// A gate scope a proof was accepted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Plan,
    Step(u32),
    Ack(u32),
}

impl Canon for Scope {
    fn canon(&self, e: &mut Encoder) {
        match self {
            Scope::Plan => e.variant("plan", 0),
            Scope::Step(n) => {
                e.variant("step", 1);
                e.u64(u64::from(*n));
            }
            Scope::Ack(n) => {
                e.variant("ack", 1);
                e.u64(u64::from(*n));
            }
        }
    }
}

macro_rules! events {
    ($( $name:ident $( { $( $field:ident : $ty:ty ),* } )? => $text:literal ,)*) => {
        /// Every journal event of section 5.10.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Event {
            $( $name $( { $( $field : $ty ),* } )? ,)*
        }

        impl Canon for Event {
            fn canon(&self, e: &mut Encoder) {
                match self {
                    $( Event::$name $( { $( $field ),* } )? => {
                        let n: u64 = 0 $( $( + { let _ = &$field; 1 } )* )?;
                        e.variant($text, n);
                        $( $( $field.canon(e); )* )?
                    } )*
                }
            }
        }
    };
}

events! {
    Checked => "checked",
    Requested => "requested",
    ProofAccepted { scope: Scope, authenticator: String, submitter: String } => "proof_accepted",
    Approved { rehearsal: bool } => "approved",
    ApprovalExpired => "approval_expired",
    Cancelled => "cancelled",
    Applying { step: u32, undo_line: String } => "applying",
    StepDone { step: u32 } => "step_done",
    StepFailed { step: u32, error: String } => "step_failed",
    Applied => "applied",
    Renewed => "renewed",
    Confirmed => "confirmed",
    Committed { by: String, reason: String } => "committed",
    Recant => "recant",
    Reverting { steps: Vec<u32> } => "reverting",
    Reverted => "reverted",
    Stuck { steps: Vec<u32> } => "stuck",
    Expired => "expired",
    Closed { reason: String } => "closed",
    BackstopFired { step: u32 } => "backstop_fired",
    BackstopFiredAfterAbandon { host: String, steps: Vec<u32> } => "backstop_fired_after_abandon",
    Held { step: u32 } => "held",
    Resumed { step: u32, by: String } => "resumed",
    Deferred { step: u32, handoff: String } => "deferred",
    HandoffDone { step: u32, by: String } => "handoff_done",
    Suspended => "suspended",
    Reestablished => "reestablished",
    Waiting { step: u32, reason: String } => "waiting",
    WaitLapsed { step: u32, reason: String } => "wait_lapsed",
    StepGateRequested { step: u32, step_digest: Hash } => "step_gate_requested",
    StepGateSatisfied { step: u32 } => "step_gate_satisfied",
    AckRequested { step: u32, cost: String } => "ack_requested",
    KnellAcknowledged { step: u32, cost: String, by: String } => "knell_acknowledged",
    Denied { gate: String, reason: String } => "denied",
    FootprintViolation { step: u32, facts: Vec<String> } => "footprint_violation",
    DriftClobbered { step: u32, facts: Vec<String> } => "drift_clobbered",
    DriftHeld { step: u32, facts: Vec<String> } => "drift_held",
    HostContractChanged { expected: Hash, observed: Hash } => "host_contract_changed",
    Refused { reason: String } => "refused",
    SecretRevealed { label: String, acceptor: String } => "secret_revealed",
    SecretUndelivered { label: String } => "secret_undelivered",
    SecretDropped { label: String, reason: String } => "secret_dropped",
    StagedRemoved { step: u32, reason: String } => "staged_removed",
    InstanceDirOrphaned { host: String, instance: String, armed: bool } => "instance_dir_orphaned",
    Reclaimed { host: String, instance: String, forced: bool, reason: String } => "reclaimed",
    Abandoned { steps_not_reverted: Vec<u32>, artifacts_left_armed: Vec<String>, by: String, reason: String } => "abandoned",
    InventoryListed { hook: String, hosts: Vec<String> } => "inventory_listed",
    HookRegistered { name: String, registrar: String, connection: String } => "hook_registered",
    HookDeregistered { name: String, registrar: String, reason: String } => "hook_deregistered",
    OperatorConnected { identity: String, admin: bool } => "operator_connected",
    OperatorDisconnected { identity: String } => "operator_disconnected",
    KeyRotated { old_pub: String, new_pub: String } => "key_rotated",
    Migrated { from: u32, to: u32, by: String } => "migrated",
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub seq: u64,
    pub prev_hash: Hash,
    pub hash: Hash,
    pub at: Instant,
    pub plan: String,
    pub instance: String,
    pub host: String,
    pub event: Event,
    /// Labels of secrets the event delivered; never values.
    pub secret_labels: Vec<String>,
    pub sig: Option<Sig>,
}

/// The canonical bytes of an entry minus its hash and signature: what the
/// hash covers and what a signature signs.
pub fn body_bytes(e: &Entry) -> Vec<u8> {
    struct Body<'a>(&'a Entry);
    impl Canon for Body<'_> {
        fn canon(&self, enc: &mut Encoder) {
            let e = self.0;
            enc.record(8);
            enc.u64(e.seq);
            e.prev_hash.canon(enc);
            enc.u64(e.at.unix_s);
            enc.str(&e.plan);
            enc.str(&e.instance);
            enc.str(&e.host);
            e.event.canon(enc);
            enc.list(&e.secret_labels);
        }
    }
    message(DOMAIN, &Body(e))
}

/// `H(prev_hash || canonical(entry without hash, sig))`.
pub fn hash_of(prev: &Hash, e: &Entry) -> Hash {
    let mut bytes = prev.0.to_vec();
    bytes.extend(body_bytes(e));
    Hash::sha256(&bytes)
}

/// The next entry of a chain: sequence and previous hash follow from the
/// last entry (or the genesis values), the hash is computed, the signature
/// slot is empty.
pub fn append(
    chain: &[Entry],
    at: Instant,
    plan: &str,
    instance: &str,
    host: &str,
    event: Event,
    secret_labels: Vec<String>,
) -> Entry {
    let (seq, prev_hash) = match chain.last() {
        Some(last) => (last.seq + 1, last.hash),
        None => (1, Hash::ZERO),
    };
    let mut e = Entry {
        seq,
        prev_hash,
        hash: Hash::ZERO,
        at,
        plan: plan.to_string(),
        instance: instance.to_string(),
        host: host.to_string(),
        event,
        secret_labels,
        sig: None,
    };
    e.hash = hash_of(&prev_hash, &e);
    e
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    /// The first entry's `prev_hash` is not the genesis value, or its seq is not 1.
    Genesis,
    /// A sequence number does not follow its predecessor.
    Seq { at_seq: u64, expected: u64 },
    /// An entry's `prev_hash` is not its predecessor's hash.
    PrevHash { seq: u64 },
    /// An entry's hash does not recompute from its bytes.
    Hash { seq: u64 },
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChainError::Genesis => write!(
                f,
                "the chain does not begin at genesis (seq 1, prev_hash zero)"
            ),
            ChainError::Seq { at_seq, expected } => write!(
                f,
                "entry {at_seq} follows {expected}; expected seq {expected}"
            ),
            ChainError::PrevHash { seq } => {
                write!(f, "entry {seq}'s prev_hash is not its predecessor's hash")
            }
            ChainError::Hash { seq } => {
                write!(f, "entry {seq}'s hash does not recompute from its bytes")
            }
        }
    }
}

impl std::error::Error for ChainError {}

/// Verify the chain end to end: genesis, consecutive sequence numbers, each
/// `prev_hash` its predecessor's hash, each hash recomputing. Signatures are
/// not checked here.
pub fn verify(chain: &[Entry]) -> Result<(), ChainError> {
    let mut prev: Option<&Entry> = None;
    for e in chain {
        match prev {
            None => {
                if e.seq != 1 || e.prev_hash != Hash::ZERO {
                    return Err(ChainError::Genesis);
                }
            }
            Some(p) => {
                if e.seq != p.seq + 1 {
                    return Err(ChainError::Seq {
                        at_seq: e.seq,
                        expected: p.seq + 1,
                    });
                }
                if e.prev_hash != p.hash {
                    return Err(ChainError::PrevHash { seq: e.seq });
                }
            }
        }
        if hash_of(&e.prev_hash, e) != e.hash {
            return Err(ChainError::Hash { seq: e.seq });
        }
        prev = Some(e);
    }
    Ok(())
}
