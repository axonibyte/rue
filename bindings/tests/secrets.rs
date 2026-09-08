//! The built-in acceptors on their own (5.13): `requester()` takes a
//! secret only while a client is attached and hands it to that client's
//! reply; `hold()` keeps one in memory, gives it up once, drops it at its
//! bound, and is emptied by a restart. Neither writes anything anywhere.

use rue_core::model::Instant;
use rue_engine::secrets::{Acceptor, Mailbox};

use rue_bindings::secrets::{Hold, Requester};

#[test]
fn requester_takes_a_secret_only_while_a_client_is_attached() {
    let mailbox = Mailbox::new();
    let mut r = Requester::new(mailbox.clone());
    // Nobody is attached: it declines, and the next acceptor is offered it.
    let now = Instant::new(0);
    assert!(!r.deliver("i-1", "a.token", "s3cr3t", now, None).unwrap());
    assert!(mailbox.drain("i-1").is_empty());
    // Attached: it takes it, and what it took is the client's to read.
    mailbox.attach(true);
    assert!(r.deliver("i-1", "a.token", "s3cr3t", now, None).unwrap());
    assert_eq!(
        mailbox.drain("i-1"),
        vec![("a.token".to_string(), "s3cr3t".to_string())]
    );
    // Drained once.
    assert!(mailbox.drain("i-1").is_empty());
    // What it holds for an instance is dropped with the instance.
    assert!(r.deliver("i-2", "b.token", "x", now, None).unwrap());
    assert_eq!(r.drop_for("i-2"), vec!["b.token".to_string()]);
    assert!(mailbox.drain("i-2").is_empty());
}

#[test]
fn hold_keeps_one_secret_in_memory_gives_it_up_once_and_drops_it_at_its_bound() {
    let mut h = Hold::new(None);
    let now = Instant::new(0);
    let bound = Instant::new(1_000);
    assert!(h
        .deliver("i-1", "a.token", "s3cr3t", now, Some(bound))
        .unwrap());
    assert!(h.holds("i-1"));
    // Once.
    assert_eq!(
        h.take("i-1"),
        Some(("a.token".to_string(), "s3cr3t".to_string()))
    );
    assert_eq!(h.take("i-1"), None);
    // The bound: before it, nothing goes; at it, the secret does.
    assert!(h.deliver("i-2", "b.token", "v", now, Some(bound)).unwrap());
    assert!(h.expire(Instant::new(999)).is_empty());
    assert_eq!(
        h.expire(Instant::new(1_000)),
        vec![("i-2".to_string(), "b.token".to_string())]
    );
    assert!(!h.holds("i-2"));
    // A restart empties it, and says what it dropped.
    assert!(h.deliver("i-3", "c.token", "v", now, None).unwrap());
    assert!(h.deliver("i-4", "d.token", "v", now, None).unwrap());
    let mut gone = h.drop_all();
    gone.sort();
    assert_eq!(
        gone,
        vec![
            ("i-3".to_string(), "c.token".to_string()),
            ("i-4".to_string(), "d.token".to_string())
        ]
    );
    assert!(!h.holds("i-3") && !h.holds("i-4"));

    // A bound of its own is honored where it is the earlier of the two.
    let mut h = Hold::new(Some(rue_core::model::Duration::new(60)));
    assert!(h
        .deliver("i-5", "e.token", "v", now, Some(Instant::new(3_600)))
        .unwrap());
    assert_eq!(
        h.expire(Instant::new(60)),
        vec![("i-5".to_string(), "e.token".to_string())],
        "hold(until: 60s) inside an hour's wane expires at the minute"
    );
}
