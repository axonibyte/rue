//! The journal chain: hashes that cover the predecessor, verification that
//! catches every kind of tampering, and the JSON spelling of an entry.

use rue_core::journal::*;
use rue_core::model::Instant;

fn chain3() -> Vec<Entry> {
    let e1 = append(
        &[],
        Instant::new(100),
        "breakglass",
        "i-1",
        "db-01",
        Event::Checked,
        vec![],
    );
    let e2 = append(
        std::slice::from_ref(&e1),
        Instant::new(101),
        "breakglass",
        "i-1",
        "db-01",
        Event::Approved { rehearsal: false },
        vec![],
    );
    let first_two = [e1, e2];
    let e3 = append(
        &first_two,
        Instant::new(102),
        "breakglass",
        "i-1",
        "db-01",
        Event::StepDone { step: 3 },
        vec!["bmc_password".into()],
    );
    let [e1, e2] = first_two;
    vec![e1, e2, e3]
}

#[test]
fn a_chain_verifies_and_its_links_are_what_the_rule_says() {
    let c = chain3();
    assert_eq!(verify(&c), Ok(()));
    assert_eq!(c[0].seq, 1);
    assert_eq!(c[0].prev_hash, Hash::ZERO);
    assert_eq!(c[1].prev_hash, c[0].hash);
    assert_eq!(c[2].prev_hash, c[1].hash);
    assert_eq!(hash_of(&c[1].prev_hash, &c[1]), c[1].hash);
    // The hash covers the predecessor: the same entry after a different
    // predecessor hashes differently.
    assert_ne!(hash_of(&Hash::ZERO, &c[1]), c[1].hash);
    assert!(c.iter().all(|e| e.sig.is_none()));
}

#[test]
fn the_empty_chain_verifies_and_every_tampering_is_named() {
    assert_eq!(verify(&[]), Ok(()));
    let c = chain3();

    let mut t = c.clone();
    t[1].event = Event::Approved { rehearsal: true };
    assert_eq!(verify(&t), Err(ChainError::Hash { seq: 2 }));

    let mut t = c.clone();
    t[2].prev_hash = Hash::ZERO;
    assert_eq!(verify(&t), Err(ChainError::PrevHash { seq: 3 }));

    let mut t = c.clone();
    t.remove(1);
    assert_eq!(
        verify(&t),
        Err(ChainError::Seq {
            at_seq: 3,
            expected: 2
        })
    );

    let t = c[1..].to_vec();
    assert_eq!(verify(&t), Err(ChainError::Genesis));

    let mut t = c.clone();
    t[2].secret_labels.clear();
    assert_eq!(verify(&t), Err(ChainError::Hash { seq: 3 }));
}

#[test]
fn every_event_encodes_distinctly_and_no_two_field_values_collide() {
    let at = Instant::new(1);
    let one = |ev: Event| append(&[], at, "p", "i", "h", ev, vec![]).hash;
    let a = one(Event::Held { step: 1 });
    let b = one(Event::Resumed {
        step: 1,
        by: "x".into(),
    });
    let c = one(Event::Reverting { steps: vec![1] });
    let d = one(Event::Stuck { steps: vec![1] });
    let e = one(Event::StepFailed {
        step: 1,
        error: String::new(),
    });
    let f = one(Event::StepDone { step: 1 });
    let all = [a, b, c, d, e, f];
    for (i, x) in all.iter().enumerate() {
        for y in &all[i + 1..] {
            assert_ne!(x, y);
        }
    }
    // A secret label lives in secret_labels, never in the event.
    let with = append(
        &[],
        at,
        "p",
        "i",
        "h",
        Event::SecretRevealed {
            label: "tok".into(),
            acceptor: "requester".into(),
        },
        vec!["tok".into()],
    );
    assert_eq!(with.secret_labels, vec!["tok".to_string()]);
}

#[test]
fn an_entry_round_trips_through_json_with_hex_hashes() {
    let c = chain3();
    let json = serde_json::to_value(&c[2]).unwrap();
    assert_eq!(
        json["event"],
        serde_json::json!({ "step_done": { "step": 3 } })
    );
    assert_eq!(json["prev_hash"].as_str().unwrap().len(), 64);
    assert_eq!(json["hash"], serde_json::Value::String(c[2].hash.to_hex()));
    let back: Entry = serde_json::from_value(json).unwrap();
    assert_eq!(back, c[2]);
    assert_eq!(Hash::from_hex(&c[2].hash.to_hex()), Some(c[2].hash));
    assert_eq!(Hash::from_hex("zz"), None);
}
