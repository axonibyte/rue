//! The journal service: entries chain through the store's copy and reach
//! every sink; a refusing sink is R0304 with the refusal journaled to the
//! rest; signatures verify with the public key and fail for any other key,
//! a tampered entry, or an unsigned one under a key.

mod common;

use rue_core::journal::{Event, Hash};
use rue_core::model::Instant;
use rue_engine::journal::{About, Journal, JournalError, MemorySink, Sink};
use rue_engine::sign::{load_public, verify_chain, verify_entry, Signer};
use rue_engine::store::Store;

fn about() -> About {
    About {
        plan: "p".into(),
        instance: "i".into(),
        host: "h".into(),
    }
}

#[test]
fn every_entry_reaches_the_store_and_every_sink_in_chain_order() {
    let d = common::TempDir::new("journal");
    let store = Store::create(&d.join("store")).unwrap();
    let a = MemorySink::new("a");
    let b = MemorySink::new("b");
    let sinks: Vec<Box<dyn Sink>> = vec![Box::new(a.clone()), Box::new(b.clone())];
    let mut j = Journal::open(&store, sinks, None).unwrap();
    let e1 = j
        .record(&store, Instant::new(10), &about(), Event::Checked, vec![])
        .unwrap();
    let e2 = j
        .record(&store, Instant::new(11), &about(), Event::Requested, vec![])
        .unwrap();
    assert_eq!((e1.seq, e1.prev_hash), (1, Hash::ZERO));
    assert_eq!((e2.seq, e2.prev_hash), (2, e1.hash));
    assert_eq!(store.read_journal().unwrap(), vec![e1.clone(), e2.clone()]);
    assert_eq!(a.entries(), vec![e1.clone(), e2.clone()]);
    assert_eq!(b.entries(), vec![e1, e2.clone()]);
    // Reopened, the chain continues from the store's tail.
    drop(j);
    let j2 = Journal::open(&store, vec![], None).unwrap();
    assert_eq!(j2.tail(), Some(&e2));
}

#[test]
fn a_refusing_sink_is_r0304_and_the_refusal_is_journaled_to_the_sinks_that_still_ack() {
    let d = common::TempDir::new("journal-refuse");
    let store = Store::create(&d.join("store")).unwrap();
    let good = MemorySink::new("good");
    let bad = MemorySink::new("bad");
    bad.refuse_with(Some("disk full"));
    let sinks: Vec<Box<dyn Sink>> = vec![Box::new(good.clone()), Box::new(bad.clone())];
    let mut j = Journal::open(&store, sinks, None).unwrap();
    let err = j
        .record(&store, Instant::new(10), &about(), Event::Applied, vec![])
        .unwrap_err();
    match &err {
        JournalError::SinkRefused { entry, refusals } => {
            assert_eq!(entry.event, Event::Applied);
            assert_eq!(
                refusals,
                &vec![("bad".to_string(), "disk full".to_string())]
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(err.to_string().starts_with("R0304"), "{err}");
    // The store holds the entry and the refusal; the good sink got both;
    // the bad sink got nothing.
    let chain = store.read_journal().unwrap();
    assert_eq!(chain.len(), 2);
    assert!(
        matches!(&chain[1].event, Event::Refused { reason } if reason.contains("R0304") && reason.contains("bad: disk full"))
    );
    assert_eq!(good.events(), vec![Event::Applied, chain[1].event.clone()]);
    assert!(bad.entries().is_empty());
    verify_chain(&chain, None).unwrap();
}

#[test]
fn signed_entries_verify_with_the_public_key_and_with_nothing_else() {
    let d = common::TempDir::new("journal-sign");
    let key = common::keypair(d.path(), "id");
    let other = common::keypair(d.path(), "other");
    let store = Store::create(&d.join("store")).unwrap();
    let signer = Signer::load(&key).unwrap();
    let mut j = Journal::open(&store, vec![], Some(signer)).unwrap();
    assert!(j.signed());
    for ev in [
        Event::Checked,
        Event::Requested,
        Event::Approved { rehearsal: false },
    ] {
        j.record(&store, Instant::new(5), &about(), ev, vec![])
            .unwrap();
    }
    let chain = store.read_journal().unwrap();
    assert!(chain.iter().all(|e| e.sig.is_some()));
    let pk = load_public(&key.with_extension("pub")).unwrap();
    verify_chain(&chain, Some(&pk)).unwrap();
    verify_chain(&chain, None).unwrap();

    // Another key: every entry fails.
    let wrong = load_public(&other.with_extension("pub")).unwrap();
    let err = verify_chain(&chain, Some(&wrong)).unwrap_err();
    assert!(
        err.contains("entry 1") && err.contains("does not verify"),
        "{err}"
    );

    // A tampered body: the hash catches it before the signature does; a
    // tampered body with a recomputed hash is caught by the signature.
    let mut tampered = chain.clone();
    tampered[1].host = "elsewhere".into();
    assert!(verify_chain(&tampered, Some(&pk))
        .unwrap_err()
        .contains("hash"));
    tampered[1].hash = rue_core::journal::hash_of(&tampered[1].prev_hash, &tampered[1]);
    tampered[2].prev_hash = tampered[1].hash;
    tampered[2].hash = rue_core::journal::hash_of(&tampered[2].prev_hash, &tampered[2]);
    let err = verify_chain(&tampered, Some(&pk)).unwrap_err();
    assert!(err.contains("entry 2") || err.contains("entry 1"), "{err}");
    assert!(err.contains("does not verify"), "{err}");

    // An unsigned entry under a key is a failure, not a pass.
    let mut unsigned = chain.clone();
    unsigned[2].sig = None;
    let err = verify_entry(&pk, &unsigned[2]).unwrap_err();
    assert!(err.contains("unsigned"), "{err}");
    assert!(verify_chain(&unsigned, Some(&pk)).is_err());
}

#[test]
fn the_signer_refuses_a_key_that_is_not_an_unencrypted_ed25519_key() {
    let d = common::TempDir::new("journal-badkey");
    std::fs::write(d.join("garbage"), "not a key\n").unwrap();
    let err = Signer::load(&d.join("garbage")).unwrap_err();
    assert!(err.contains("garbage"), "{err}");
    let ecdsa = ssh_key::PrivateKey::random(
        &mut rand_core::OsRng,
        ssh_key::Algorithm::Ecdsa {
            curve: ssh_key::EcdsaCurve::NistP256,
        },
    )
    .unwrap();
    ecdsa
        .write_openssh_file(&d.join("ecdsa"), ssh_key::LineEnding::LF)
        .unwrap();
    let err = Signer::load(&d.join("ecdsa")).unwrap_err();
    assert!(err.contains("not Ed25519"), "{err}");
    // And a generated key round-trips through the file it wrote.
    let made = rue_engine::sign::generate(&d.join("made")).unwrap();
    assert_eq!(
        Signer::load(&d.join("made")).unwrap().public_openssh(),
        made.public_openssh()
    );
    assert_eq!(
        std::fs::read_to_string(d.join("made.pub")).unwrap().trim(),
        made.public_openssh()
    );
}
