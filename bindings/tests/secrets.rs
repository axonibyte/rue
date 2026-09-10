//! The built-in secret bindings on their own (5.13). The acceptors:
//! `requester()` takes a secret only while a client is attached and hands
//! it to that client's reply; `hold()` keeps one in memory, gives it up
//! once, drops it at its bound, and is emptied by a restart. Neither
//! writes anything anywhere. And the source: `file()` reads a TOML table
//! and refuses one anyone else on the host can read.

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

#[cfg(unix)]
mod file_source {
    // `file()` is unix-only for now: what it refuses is a mode, and the
    // Windows equivalent is an ACL check that waits for Phase 3W. So the
    // import lives here rather than at file scope, where it would be
    // unused on the Windows target and `-D warnings` would say so.
    use rue_bindings::secrets::FileSource;
    use rue_engine::secrets::Source;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn write(name: &str, body: &str, mode: u32) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rue-secrets-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&p, body).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn a_reference_resolves_and_one_the_file_does_not_hold_is_refused() {
        let p = write("ok", "db_pw = \"s3cr3t\"\napi = \"k\"\n", 0o600);
        let mut src = FileSource::new(&p);
        assert_eq!(src.resolve("db_pw").unwrap(), "s3cr3t");
        assert_eq!(src.resolve("api").unwrap(), "k");
        // Not an empty string: a blank where a credential belongs would
        // run the step and look like it worked.
        let e = src.resolve("absent").unwrap_err().to_string();
        assert!(e.contains("absent"), "{e}");
        fs::remove_file(&p).ok();
    }

    #[test]
    fn a_secrets_file_others_can_read_is_refused_rather_than_read() {
        // The failure this prevents is silent: the plan runs perfectly
        // while the credential is world-readable, and nothing says so.
        for mode in [0o644, 0o640, 0o604] {
            let p = write("mode", "db_pw = \"s3cr3t\"\n", mode);
            let mut src = FileSource::new(&p);
            let e = src.resolve("db_pw").unwrap_err().to_string();
            assert!(
                e.contains("not a secret") && e.contains(&format!("{mode:04o}")),
                "mode {mode:04o}: {e}"
            );
            assert!(!e.contains("s3cr3t"), "the refusal quoted the file: {e}");
            fs::remove_file(&p).ok();
        }
        // 0600 and 0400 are fine.
        for mode in [0o600, 0o400] {
            let p = write("mode-ok", "db_pw = \"s3cr3t\"\n", mode);
            assert_eq!(FileSource::new(&p).resolve("db_pw").unwrap(), "s3cr3t");
            fs::remove_file(&p).ok();
        }
    }

    #[test]
    fn a_file_that_is_not_there_is_a_refusal_naming_it() {
        let mut src = FileSource::new("/nonexistent/rue-secrets.toml");
        let e = src.resolve("db_pw").unwrap_err().to_string();
        assert!(e.contains("rue-secrets.toml"), "{e}");
    }
}
