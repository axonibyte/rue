//! Gates at runtime, docs/ROADMAP.md 5.11 and 7.5: the request digest and
//! its scoped forms, the approval binding that renders a challenge over a
//! digest and verifies proofs against it, the proofs an instance
//! accumulates, and the host contract the digest covers.
//!
//! Rue supplies a digest, not a challenge. What a proof is, and how a
//! human produces it, belongs to the binding; what rue guarantees is that
//! a proof is bound to one request and one scope, so a proof for a step
//! verifies for no other step and none for the plan, and that a change to
//! the plan, its parameters or the host contract after the request
//! invalidates every proof already given (R0301).

use std::collections::BTreeMap;

use rue_core::journal::{Event as J, Hash, Scope};
use rue_core::model::{Authenticator, Instant};
use rue_core::request::{hash_json, request_digest, scoped_digest, Request};
use rue_core::states::State;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::executor::{ExecError, ProbeRun};
use crate::lifecycle::{Engine, EngineError, InstanceRecord};

/// One proof an instance has accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub scope: Scope,
    /// The authenticator the binding verified the proof against.
    pub authenticator: String,
    /// The operator who submitted it, from the channel's peer identity.
    pub submitter: String,
    pub at: Instant,
}

/// What the binding is asked to challenge or verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofRequest {
    pub instance: String,
    pub digest: Hash,
    pub scope: Scope,
    /// What the operator is being asked to approve, in words.
    pub context: String,
    pub authenticator: String,
    pub proof: String,
}

/// The binding's answer to `verify`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub verified: bool,
    pub reason: String,
}

/// `approval via:` (7.3): the authenticators it publishes, the challenge
/// it renders over a digest, and its verdict on a proof.
pub trait Approval: Send {
    fn name(&self) -> &str;
    fn authenticators(&mut self) -> Result<Vec<Authenticator>, ExecError>;
    fn challenge(&mut self, r: &ProofRequest) -> Result<String, ExecError>;
    fn verify(&mut self, r: &ProofRequest) -> Result<Verified, ExecError>;
    /// A binding that opens every gate without a proof. Only `always()`
    /// answers yes, and the daemon admits it only in dry-run mode: a
    /// rehearsal evaluates and journals its gates and reserves nothing.
    fn approves_everything(&self) -> bool {
        false
    }
}

/// The host contract (5.1): the `HostRecord` fields of every host a plan
/// touches and the value of every probe the plan declares `static: true`
/// on those hosts, frozen at the request. A change to any of it after the
/// request is R0301, and every proof accumulated falls with it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HostContract {
    pub hosts: BTreeMap<String, serde_json::Value>,
    /// `<host>/<probe>` to the text the probe answered.
    pub statics: BTreeMap<String, String>,
}

impl HostContract {
    pub fn hash(&self) -> Hash {
        hash_json(&json!({ "hosts": self.hosts, "statics": self.statics })).unwrap_or(Hash([0; 32]))
    }
}

/// 32 bytes from the platform's CSPRNG: the request nonce core will not
/// draw for itself.
pub fn nonce() -> [u8; 32] {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("the platform CSPRNG");
    b
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

impl Engine {
    /// Every host the plan touches, by name.
    fn touched_hosts(&self, rec: &InstanceRecord) -> Vec<String> {
        let mut v: Vec<String> = vec![rec.plan().owner.clone()];
        for (n, _) in rue_core::algebra::numbered(&rec.plan().body) {
            if let Some(op) = rec.op_at(n) {
                if let Ok(h) = self.step_host(rec, op) {
                    v.push(h.name().to_string());
                }
            }
        }
        v.sort();
        v.dedup();
        v
    }

    /// Derive the host contract now: the records as the site holds them
    /// and every `static: true` probe observed on its host.
    pub(crate) fn host_contract(&mut self, rec: &InstanceRecord) -> HostContract {
        let mut c = HostContract::default();
        for name in self.touched_hosts(rec) {
            let Some(host) = self.host_of(&name) else {
                continue;
            };
            if let Ok(v) = serde_json::to_value(&host.record) {
                c.hosts.insert(name.clone(), v);
            }
            if rec.rehearsal {
                continue;
            }
            for p in rec.plan().probes.iter().filter(|p| p.static_) {
                let run = ProbeRun {
                    name: p.name.clone(),
                    body: Vec::new(),
                };
                let Some(i) = self.executor_index(&host) else {
                    continue;
                };
                if let Ok(o) = self.executors[i].observe(&host, &run) {
                    c.statics.insert(format!("{name}/{}", p.name), o.text);
                }
            }
        }
        c
    }

    /// R0301: the host contract as it is now against the one the request
    /// froze. `Some((expected, observed))` when they differ.
    pub(crate) fn contract_changed(&mut self, rec: &InstanceRecord) -> Option<(Hash, Hash)> {
        let expected = match unhex(&rec.host_contract) {
            Some(b) if b.len() == 32 => {
                let mut h = [0u8; 32];
                h.copy_from_slice(&b);
                Hash(h)
            }
            _ => return None,
        };
        let observed = self.host_contract(rec).hash();
        if observed == expected {
            None
        } else {
            Some((expected, observed))
        }
    }

    /// Refuse an instance whose host contract has changed since its
    /// request, journaling what changed (R0301). `true` when it did.
    pub(crate) fn refuse_on_contract_change(
        &mut self,
        rec: &mut InstanceRecord,
    ) -> Result<bool, EngineError> {
        if rec.rehearsal || rec.host_contract.is_empty() {
            return Ok(false);
        }
        let Some((expected, observed)) = self.contract_changed(rec) else {
            return Ok(false);
        };
        self.log(rec, J::HostContractChanged { expected, observed })?;
        rec.proofs.clear();
        rec.refusal = Some(
            "R0301: the host contract changed since the request; every proof falls with it".into(),
        );
        self.step(rec, rue_core::states::Event::HostContractChanged)?;
        self.log(
            rec,
            J::Refused {
                reason: rec.refusal.clone().unwrap_or_default(),
            },
        )?;
        if rec.state == State::Closed {
            rec.closed_reason = rec.refusal.clone();
            self.log(
                rec,
                J::Closed {
                    reason: rec.closed_reason.clone().unwrap_or_default(),
                },
            )?;
            self.release(rec)?;
        }
        self.persist(rec)?;
        Ok(true)
    }

    /// The request digest of an instance: what every proof binds to.
    pub(crate) fn digest_of(&self, rec: &InstanceRecord) -> Hash {
        let mut n = [0u8; 32];
        if let Some(b) = unhex(&rec.nonce) {
            if b.len() == 32 {
                n.copy_from_slice(&b);
            }
        }
        let contract = match unhex(&rec.host_contract) {
            Some(b) if b.len() == 32 => {
                let mut h = [0u8; 32];
                h.copy_from_slice(&b);
                Hash(h)
            }
            _ => Hash([0; 32]),
        };
        let r = Request {
            nonce: n,
            plan_id: rec.plan().id.clone(),
            instance: rec.id.clone(),
            owner_host: rec.plan().owner.clone(),
            params_hash: hash_json(&json!(rec.params)).unwrap_or(Hash([0; 32])),
            host_contract_hash: contract,
            wane: rue_core::intent::effective_wane(rec.plan()),
            requested_at: rec.requested_at.unwrap_or(Instant::new(0)),
            gate_hash: serde_json::to_value(&rec.plan().gate)
                .ok()
                .and_then(|v| hash_json(&v).ok())
                .unwrap_or(Hash([0; 32])),
            plan_content_hash: serde_json::to_value(rec.plan())
                .ok()
                .and_then(|v| hash_json(&v).ok())
                .unwrap_or(Hash([0; 32])),
        };
        request_digest(&r)
    }

    /// The digest a proof for a scope binds to.
    pub fn scope_digest(&self, rec: &InstanceRecord, scope: Scope) -> Hash {
        scoped_digest(&self.digest_of(rec), scope)
    }

    /// The authenticators the approval binding publishes, empty with no
    /// binding.
    pub(crate) fn authenticators(&mut self) -> Vec<Authenticator> {
        match self.approval.as_mut() {
            Some(a) => a.authenticators().unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// The proofs an instance holds for a scope, by authenticator.
    pub(crate) fn proofs_for(&self, rec: &InstanceRecord, scope: Scope) -> Vec<String> {
        rec.proofs
            .iter()
            .filter(|p| p.scope == scope)
            .map(|p| p.authenticator.clone())
            .collect()
    }

    /// Whether a gate is satisfied now, given the proofs for its scope and
    /// the time since the request (a wait factor is weight that accrues).
    pub(crate) fn gate_satisfied(
        &mut self,
        rec: &InstanceRecord,
        scope: Scope,
        gate: &rue_core::model::GateExpr,
    ) -> bool {
        let auths = self.authenticators();
        let proofs = self.proofs_for(rec, scope);
        let elapsed = rue_core::model::Duration::new(
            self.clock
                .now()
                .unix_s
                .saturating_sub(rec.requested_at.map(|i| i.unix_s).unwrap_or(0)),
        );
        if self
            .approval
            .as_ref()
            .is_some_and(|a| a.approves_everything())
        {
            return true;
        }
        rue_core::gates::satisfied(&auths, gate, &proofs, elapsed)
    }

    /// `rue approve`: the challenge the binding renders for a scope, for
    /// an operator about to produce a proof.
    pub fn challenge(
        &mut self,
        id: &str,
        scope: Scope,
        context: &str,
    ) -> Result<String, EngineError> {
        let rec = self.load(id)?;
        let digest = self.scope_digest(&rec, scope);
        let r = ProofRequest {
            instance: rec.id.clone(),
            digest,
            scope,
            context: context.to_string(),
            authenticator: String::new(),
            proof: String::new(),
        };
        match self.approval.as_mut() {
            Some(a) => a
                .challenge(&r)
                .map_err(|e| EngineError::Runtime(format!("R0302: the approval binding: {e}"))),
            None => Err(EngineError::Runtime(
                "the site declares no approval binding".into(),
            )),
        }
    }

    /// `rue approve <instance> [--step N]`: verify a proof, record it, and
    /// let the instance proceed if its gate is now satisfied.
    pub fn approve_proof(
        &mut self,
        id: &str,
        scope: Scope,
        authenticator: &str,
        proof: &str,
        submitter: &str,
    ) -> Result<crate::lifecycle::Outcome, EngineError> {
        let mut rec = self.load(id)?;
        // Every proof is bound to the contract the request froze: a change
        // invalidates the proofs already given and refuses the instance.
        if self.refuse_on_contract_change(&mut rec)? {
            return Ok(self.outcome(&rec));
        }
        let digest = self.scope_digest(&rec, scope);
        let r = ProofRequest {
            instance: rec.id.clone(),
            digest,
            scope,
            context: String::new(),
            authenticator: authenticator.to_string(),
            proof: proof.to_string(),
        };
        let verdict = match self.approval.as_mut() {
            Some(a) => a
                .verify(&r)
                .map_err(|e| EngineError::Runtime(format!("R0302: the approval binding: {e}")))?,
            None => {
                return Err(EngineError::Runtime(
                    "the site declares no approval binding".into(),
                ))
            }
        };
        if !verdict.verified {
            self.log(
                &rec,
                J::Denied {
                    gate: match scope {
                        Scope::Plan => "plan".into(),
                        Scope::Step(n) => format!("step {n}"),
                        Scope::Ack(n) => format!("ack {n}"),
                    },
                    reason: verdict.reason.clone(),
                },
            )?;
            return Err(EngineError::Runtime(format!(
                "the proof from {authenticator} was not accepted: {}",
                verdict.reason
            )));
        }
        let at = self.clock.now();
        if !rec
            .proofs
            .iter()
            .any(|p| p.scope == scope && p.authenticator == authenticator)
        {
            rec.proofs.push(Proof {
                scope,
                authenticator: authenticator.to_string(),
                submitter: submitter.to_string(),
                at,
            });
        }
        self.log(
            &rec,
            J::ProofAccepted {
                scope,
                authenticator: authenticator.to_string(),
                submitter: submitter.to_string(),
            },
        )?;
        self.persist(&rec)?;
        self.after_proof(&mut rec, scope)?;
        Ok(self.outcome(&rec))
    }

    /// A proof may be the one that opens a gate: the plan's, at Pending,
    /// or a step's, at Waiting.
    fn after_proof(&mut self, rec: &mut InstanceRecord, scope: Scope) -> Result<(), EngineError> {
        match scope {
            Scope::Plan if rec.state == State::Pending => {
                let gate = rec.plan().gate.as_ref().map(|g| g.expr.clone());
                if let Some(g) = gate {
                    if self.gate_satisfied(rec, Scope::Plan, &g) {
                        self.approve(rec)?;
                        self.persist(rec)?;
                        self.walk(rec)?;
                    }
                }
            }
            Scope::Step(n) if rec.state == State::Waiting => {
                let gate = rec.step_at(n).and_then(|s| s.gate.clone());
                if let Some(g) = gate {
                    if self.gate_satisfied(rec, Scope::Step(n), &g) {
                        self.step(rec, rue_core::states::Event::WaitSatisfied)?;
                        rec.waiting = None;
                        self.log(rec, J::StepGateSatisfied { step: n })?;
                        self.persist(rec)?;
                        self.walk(rec)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `rue ack --step N --reason`: the acknowledgement a knell waits for,
    /// journaled under the operator who gave it.
    pub fn ack(
        &mut self,
        id: &str,
        step: u32,
        reason: &str,
        proof: &str,
        by: &str,
    ) -> Result<crate::lifecycle::Outcome, EngineError> {
        let mut rec = self.load(id)?;
        if reason.trim().is_empty() {
            return Err(EngineError::Runtime(
                "an acknowledgement needs a reason".into(),
            ));
        }
        // A knell whose `ack:` is a gate needs a proof in the ack scope,
        // verified like any other: the reason is for the journal, the
        // token is what makes it an acknowledgement (5.11).
        let ack_gate = rec.step_at(step).and_then(|s| match &s.op.refusal {
            rue_core::model::Refusal::Knell {
                ack: rue_core::model::Ack::Gate(g),
                ..
            } => Some(g.clone()),
            _ => None,
        });
        if let Some(g) = ack_gate {
            let digest = self.scope_digest(&rec, Scope::Ack(step));
            let r = ProofRequest {
                instance: rec.id.clone(),
                digest,
                scope: Scope::Ack(step),
                context: reason.to_string(),
                authenticator: by.to_string(),
                proof: proof.to_string(),
            };
            let verdict = match self.approval.as_mut() {
                Some(a) => a.verify(&r).map_err(|e| {
                    EngineError::Runtime(format!("R0302: the approval binding: {e}"))
                })?,
                None => Verified {
                    verified: false,
                    reason: "the site declares no approval binding".into(),
                },
            };
            if !verdict.verified {
                self.log(
                    &rec,
                    J::Denied {
                        gate: format!("ack {step}"),
                        reason: verdict.reason.clone(),
                    },
                )?;
                return Err(EngineError::Runtime(format!(
                    "the acknowledgement from {by} was not accepted: {}",
                    verdict.reason
                )));
            }
            let mut proofs = self.proofs_for(&rec, Scope::Ack(step));
            proofs.push(by.to_string());
            let auths = self.authenticators();
            let elapsed = rue_core::model::Duration::new(
                self.clock
                    .now()
                    .unix_s
                    .saturating_sub(rec.requested_at.map(|i| i.unix_s).unwrap_or(0)),
            );
            let opens = self
                .approval
                .as_ref()
                .is_some_and(|a| a.approves_everything())
                || rue_core::gates::satisfied(&auths, &g, &proofs, elapsed);
            if !opens {
                // The proof is recorded; the gate is not yet open.
                rec.proofs.push(Proof {
                    scope: Scope::Ack(step),
                    authenticator: by.to_string(),
                    submitter: by.to_string(),
                    at: self.clock.now(),
                });
                self.log(
                    &rec,
                    J::ProofAccepted {
                        scope: Scope::Ack(step),
                        authenticator: by.to_string(),
                        submitter: by.to_string(),
                    },
                )?;
                self.persist(&rec)?;
                return Ok(self.outcome(&rec));
            }
        }
        let cost = rec
            .step_at(step)
            .map(|s| match &s.op.refusal {
                rue_core::model::Refusal::Knell { cost, .. } => match cost {
                    rue_core::model::Cost::Probe(p) => p.clone(),
                    rue_core::model::Cost::NoCost(_) => "none".into(),
                },
                _ => "none".into(),
            })
            .unwrap_or_else(|| "none".into());
        if !rec.acks.contains(&step) {
            rec.acks.push(step);
        }
        rec.proofs.push(Proof {
            scope: Scope::Ack(step),
            authenticator: by.to_string(),
            submitter: by.to_string(),
            at: self.clock.now(),
        });
        self.log(
            &rec,
            J::KnellAcknowledged {
                step,
                cost,
                by: format!("{by}: {reason}"),
            },
        )?;
        self.persist(&rec)?;
        // An instance waiting on this acknowledgement goes on.
        if rec.state == State::Waiting
            && rec.waiting.as_ref().map(|w| (w.step, w.reason.as_str())) == Some((step, "ack"))
        {
            self.step(&mut rec, rue_core::states::Event::WaitSatisfied)?;
            rec.waiting = None;
            self.persist(&rec)?;
            self.walk(&mut rec)?;
        }
        Ok(self.outcome(&rec))
    }
}

// ---------------------------------------------------------------------------
// The fake: scripted verdicts, every call recorded.

use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub struct FakeApproval {
    pub auths: Vec<Authenticator>,
    /// The binding itself fails, rather than refusing a proof (R0302).
    pub broken: bool,
    /// Authenticators whose proofs are refused, with the reason.
    pub refuse: BTreeMap<String, String>,
    pub calls: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FakeApprovalHandle(pub Arc<Mutex<FakeApproval>>);

impl FakeApprovalHandle {
    pub fn new(auths: Vec<Authenticator>) -> FakeApprovalHandle {
        let h = FakeApprovalHandle::default();
        h.with(|f| f.auths = auths);
        h
    }
    pub fn with<T>(&self, f: impl FnOnce(&mut FakeApproval) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn calls(&self) -> Vec<String> {
        self.with(|f| f.calls.clone())
    }
    pub fn refuse(&self, auth: &str, why: &str) {
        self.with(|f| {
            f.refuse.insert(auth.to_string(), why.to_string());
        });
    }
}

impl Approval for FakeApprovalHandle {
    fn name(&self) -> &str {
        "fake"
    }
    fn authenticators(&mut self) -> Result<Vec<Authenticator>, ExecError> {
        Ok(self.with(|f| f.auths.clone()))
    }
    fn challenge(&mut self, r: &ProofRequest) -> Result<String, ExecError> {
        if self.with(|f| f.broken) {
            return Err(ExecError::Failed("the fake binding is broken".into()));
        }
        self.with(|f| {
            f.calls
                .push(format!("challenge {:?} {}", r.scope, hex(&r.digest.0[..4])));
        });
        Ok(format!(
            "approve {} [{}]",
            r.instance,
            hex(&r.digest.0[..4])
        ))
    }
    fn verify(&mut self, r: &ProofRequest) -> Result<Verified, ExecError> {
        if self.with(|f| f.broken) {
            return Err(ExecError::Failed("the fake binding is broken".into()));
        }
        self.with(|f| {
            f.calls.push(format!(
                "verify {:?} {} {}",
                r.scope,
                r.authenticator,
                hex(&r.digest.0[..4])
            ));
            match f.refuse.get(&r.authenticator) {
                Some(why) => Ok(Verified {
                    verified: false,
                    reason: why.clone(),
                }),
                None => Ok(Verified {
                    verified: true,
                    reason: String::new(),
                }),
            }
        })
    }
}
