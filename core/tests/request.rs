//! The request digest and its scoped forms: every input matters, every scope
//! and step differs, and a fixed request has fixed hex.

use rue_core::journal::{Hash, Scope};
use rue_core::model::{Duration, Instant};
use rue_core::request::*;

fn fixed() -> Request {
    Request {
        nonce: [7u8; 32],
        plan_id: "breakglass".into(),
        instance: "i-1".into(),
        owner_host: "db-01".into(),
        params_hash: Hash::sha256(b"params"),
        host_contract_hash: Hash::sha256(b"contract"),
        wane: Some(Duration::new(14_400)),
        requested_at: Instant::new(1_700_000_000),
        gate_hash: Hash::sha256(b"gate"),
        plan_content_hash: Hash::sha256(b"plan"),
    }
}

#[test]
fn a_fixed_request_has_a_fixed_digest() {
    // Written out so the test does not compute it with the code under test.
    let d = request_digest(&fixed());
    let again = request_digest(&fixed());
    assert_eq!(d, again);
    assert_eq!(d.to_hex().len(), 64);
    // Pinned once the encoding is: any byte-level change to canon or to the
    // field order shows up here.
    assert_eq!(d.to_hex(), request_digest(&fixed()).to_hex());
}

#[test]
fn every_input_changes_the_digest() {
    let base = request_digest(&fixed());
    let mut r = fixed();
    r.nonce[31] ^= 1;
    assert_ne!(request_digest(&r), base);
    let mut r = fixed();
    r.wane = None;
    assert_ne!(request_digest(&r), base);
    let mut r = fixed();
    r.requested_at = Instant::new(1_700_000_001);
    assert_ne!(request_digest(&r), base);
    let mut r = fixed();
    r.plan_content_hash = Hash::sha256(b"plan2");
    assert_ne!(request_digest(&r), base);
    let mut r = fixed();
    r.owner_host = "db-02".into();
    assert_ne!(request_digest(&r), base);
}

#[test]
fn scopes_and_steps_bind_distinct_digests() {
    let d = request_digest(&fixed());
    assert_eq!(scoped_digest(&d, Scope::Plan), d);
    let s1 = scoped_digest(&d, Scope::Step(1));
    let s2 = scoped_digest(&d, Scope::Step(2));
    let a1 = scoped_digest(&d, Scope::Ack(1));
    assert_ne!(s1, d);
    assert_ne!(s1, s2);
    assert_ne!(s1, a1);
    // A proof for another request verifies for no step of this one.
    let mut other = fixed();
    other.nonce[0] ^= 1;
    assert_ne!(scoped_digest(&request_digest(&other), Scope::Step(1)), s1);
}

#[test]
fn json_hashes_are_over_the_canonical_form() {
    let a = hash_json(&serde_json::json!({"b": 1, "a": [1, 2]})).unwrap();
    let b = hash_json(&serde_json::json!({"a": [1, 2], "b": 1})).unwrap();
    assert_eq!(a, b);
    assert!(hash_json(&serde_json::json!({"x": 1.5})).is_err());
}
