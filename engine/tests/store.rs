//! The store: locked (a second opener is refused), atomic (a record is
//! whole or absent, never half), versioned (an unknown schema is R0502 and
//! nothing is migrated silently); migration is explicit, dry-runnable, and
//! refuses a store owned by another account.

mod common;

use std::collections::BTreeMap;
use std::fs;

use rue_core::interference::Fact;
use rue_core::ledger::{Instance, Ledger};
use rue_engine::store::{migrate, schema_of, SchemaError, Store, StoreError, SCHEMA};

#[test]
fn a_created_store_carries_the_schema_and_a_second_opener_is_refused() {
    let d = common::TempDir::new("store");
    let root = d.join("store");
    let s = Store::create(&root).unwrap();
    assert_eq!(schema_of(&root), Ok(SCHEMA));
    assert!(root.join("instances").is_dir());
    match Store::open(&root) {
        Err(StoreError::Locked(p)) => assert_eq!(p, root),
        other => panic!("a second opener got {other:?}"),
    }
    drop(s);
    Store::open(&root).unwrap();
    // Creating over an existing store is refused rather than reset.
    assert!(matches!(Store::create(&root), Err(StoreError::Corrupt(_))));
}

#[test]
fn records_round_trip_and_a_crash_mid_write_leaves_the_previous_record() {
    let d = common::TempDir::new("store-atomic");
    let s = Store::create(&d.join("store")).unwrap();
    let mut rec = BTreeMap::new();
    rec.insert("state".to_string(), "Applying".to_string());
    s.write_instance("i-1", &rec).unwrap();
    assert_eq!(
        s.read_instance::<BTreeMap<String, String>>("i-1").unwrap(),
        Some(rec.clone())
    );
    assert_eq!(s.instance_ids().unwrap(), vec!["i-1".to_string()]);
    // A temporary file left by a crash before the rename is not a record.
    fs::write(d.join("store/instances/i-1.rue-tmp"), b"{\"state\": \"half").unwrap();
    assert_eq!(
        s.read_instance::<BTreeMap<String, String>>("i-1").unwrap(),
        Some(rec)
    );
    assert_eq!(s.instance_ids().unwrap(), vec!["i-1".to_string()]);
    s.remove_instance("i-1").unwrap();
    assert_eq!(
        s.read_instance::<BTreeMap<String, String>>("i-1").unwrap(),
        None
    );
    s.remove_instance("i-1").unwrap();
}

#[test]
fn the_ledger_round_trips_with_its_reservations_intact() {
    let d = common::TempDir::new("store-ledger");
    let s = Store::create(&d.join("store")).unwrap();
    let l = Ledger::new()
        .request(Instance {
            id: "a".into(),
            host: "h".into(),
            umbra: vec![Fact::new("file:/x", Some("blk"))],
            exclusivity: Some("cls".into()),
            rehearsal: false,
        })
        .unwrap()
        .request(Instance {
            id: "b".into(),
            host: "h".into(),
            umbra: vec![Fact::new("file:/y", None)],
            exclusivity: None,
            rehearsal: false,
        })
        .unwrap();
    s.write_ledger(&l).unwrap();
    let back = s.read_ledger().unwrap();
    assert_eq!(back.holdings(), l.holdings());
    // The reservations still bite after the round trip.
    let err = back
        .request(Instance {
            id: "c".into(),
            host: "h".into(),
            umbra: vec![Fact::new("file:/x", Some("blk"))],
            exclusivity: None,
            rehearsal: false,
        })
        .unwrap_err();
    assert_eq!(err.0, rue_core::ledger::LedgerCode::R0203);
    let empty = Store::create(&d.join("empty")).unwrap();
    assert!(empty.read_ledger().unwrap().holdings().is_empty());
}

#[test]
fn an_unknown_or_missing_schema_is_r0502_and_is_never_migrated_silently() {
    let d = common::TempDir::new("store-schema");
    let root = d.join("store");
    fs::create_dir_all(&root).unwrap();
    // Schema 0: a store from before schemas.
    match Store::open(&root) {
        Err(StoreError::Schema(SchemaError::Missing)) => {}
        other => panic!("{other:?}"),
    }
    assert!(Store::open(&root)
        .unwrap_err()
        .to_string()
        .starts_with("R0502"));
    assert!(!root.join("schema").exists(), "open wrote a schema");
    // A newer schema.
    fs::write(root.join("schema"), "99\n").unwrap();
    match Store::open(&root) {
        Err(StoreError::Schema(SchemaError::Unknown(99))) => {}
        other => panic!("{other:?}"),
    }
    let err = migrate(&root, false, "me").unwrap_err();
    assert!(
        matches!(err, StoreError::Schema(SchemaError::Unknown(99))),
        "{err}"
    );
    // Garbage.
    fs::write(root.join("schema"), "one\n").unwrap();
    assert!(matches!(
        Store::open(&root),
        Err(StoreError::Schema(SchemaError::Unreadable(_)))
    ));
}

#[test]
fn migration_from_schema_zero_is_dry_runnable_explicit_and_recorded_for_the_next_start() {
    let d = common::TempDir::new("store-migrate");
    let root = d.join("store");
    fs::create_dir_all(root.join("instances")).unwrap();
    fs::write(root.join("instances/old.json"), b"{}\n").unwrap();
    let dry = migrate(&root, true, "admin").unwrap();
    assert_eq!((dry.from, dry.to, dry.dry_run), (0, SCHEMA, true));
    assert!(
        dry.steps.iter().any(|s| s.contains("write schema")),
        "{:?}",
        dry.steps
    );
    assert!(!root.join("schema").exists(), "a dry run wrote");
    assert!(!root.join("migrated.json").exists());

    let m = migrate(&root, false, "admin").unwrap();
    assert_eq!((m.from, m.to, m.dry_run), (0, SCHEMA, false));
    assert_eq!(schema_of(&root), Ok(SCHEMA));
    let s = Store::open(&root).unwrap();
    let rec = s.read_meta("migrated").unwrap();
    assert_eq!(rec.get("from").map(String::as_str), Some("0"));
    assert_eq!(
        rec.get("to").map(String::as_str),
        Some(&*SCHEMA.to_string())
    );
    assert_eq!(rec.get("by").map(String::as_str), Some("admin"));
    assert_eq!(
        s.instance_ids().unwrap(),
        vec!["old".to_string()],
        "instances survive"
    );
    drop(s);
    let again = migrate(&root, false, "admin").unwrap();
    assert!(
        again.steps[0].contains("nothing to do"),
        "{:?}",
        again.steps
    );
    assert!(migrate(&d.join("nowhere"), true, "admin").is_err());
}

#[cfg(unix)]
#[test]
fn migration_refuses_a_store_owned_by_another_account() {
    // Only root can create a directory owned by someone else; as anyone else
    // the check is proven by a directory whose owner is not the caller:
    // /, which is root's.
    use std::os::unix::fs::MetadataExt;
    if unsafe_geteuid() == 0 {
        eprintln!("running as root: every directory is ours; the refusal is exercised on a guest as a user");
        return;
    }
    let owner = fs::metadata("/").unwrap().uid();
    assert_ne!(owner, unsafe_geteuid());
    match migrate(std::path::Path::new("/"), true, "me") {
        Err(StoreError::NotOwned { owner: o, .. }) => assert_eq!(o, owner),
        other => panic!("{other:?}"),
    }
}

#[cfg(unix)]
fn unsafe_geteuid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}
