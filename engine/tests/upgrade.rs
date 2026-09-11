//! Upgrade vectors for the store (ROADMAP 7.13, Phase 5; docs/issues/0001):
//! a store each release wrote with its own engine (tests/fixtures/
//! store-<release>), migrated to this build's schema and driven by this
//! build's engine. v0.1.0's holds an instance applied across a repeat
//! (store.rs holds its migration in detail); v0.2.0's, written by v0.2.0's
//! engine for this test, an instance of two steps restored by footprint,
//! with their snapshots and markers. `rued migrate` is what an operator runs
//! between releases; this is what it has to leave behind.

mod common;

use std::path::{Path, PathBuf};

use common::world::World;
use common::TempDir;
use rue_core::states::State;
use rue_engine::store::{migrate, schema_of, Store, SCHEMA};

fn fixture(release: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("store-{release}"))
}

/// A copy of a release's store in a directory of the test's own.
fn copy_of(release: &str) -> TempDir {
    let dir = TempDir::new(&format!("upgrade-{release}"));
    let from = fixture(release);
    let to = dir.join("store");
    std::fs::create_dir_all(to.join("instances")).unwrap();
    for rel in ["schema", "ledger.json", "journal.ndjson"] {
        std::fs::copy(from.join(rel), to.join(rel)).unwrap();
    }
    for e in std::fs::read_dir(from.join("instances")).unwrap() {
        let p = e.unwrap().path();
        std::fs::copy(&p, to.join("instances").join(p.file_name().unwrap())).unwrap();
    }
    dir
}

fn a_released_store_migrates_and_drives(release: &str, schema: u32, id: &str) {
    let dir = copy_of(release);
    let root = dir.join("store");
    assert_eq!(
        schema_of(&root),
        Ok(schema),
        "{release}'s store is schema {schema}"
    );
    // This build refuses it until it is migrated, rather than guessing.
    assert!(
        Store::open(&root).is_err(),
        "{release}'s store opened unmigrated"
    );
    let m = migrate(&root, false, "the upgrade test").unwrap();
    assert_eq!((m.from, m.to), (schema, SCHEMA));
    assert_eq!(schema_of(&root), Ok(SCHEMA));

    let mut w = World::over(dir);
    w.engine.boot().unwrap();
    let rec = w
        .engine
        .status(id)
        .unwrap()
        .expect("the release's instance");
    assert_eq!(rec.state, State::Applied, "{rec:?}");
    assert!(!rec.applied.is_empty());
    // Driven by this build: recanted, its steps undone, closed.
    let out = w.engine.recant(id, &[]).unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    // The chain the release began and this build continued is one chain.
    let chain = w.engine.store().read_journal().unwrap();
    let before = std::fs::read_to_string(fixture(release).join("journal.ndjson"))
        .unwrap()
        .lines()
        .count();
    assert!(
        chain.len() > before,
        "this build appended to the release's journal"
    );
    rue_core::journal::verify(&chain).unwrap_or_else(|e| panic!("{release}: {e:?}"));
}

#[test]
fn the_first_release_s_store_migrates_and_its_instance_is_driven_by_this_build() {
    a_released_store_migrates_and_drives("v0.1.0", 1, "vector.h.96925251");
}

#[test]
fn the_second_release_s_store_migrates_and_its_instance_is_driven_by_this_build() {
    a_released_store_migrates_and_drives("v0.2.0", 2, "upgrade.h.ca3d163b");
}
