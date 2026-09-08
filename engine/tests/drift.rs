//! Footprints and drift over the fake executor's file facts (5.2, 7.7): the
//! instance directory is created before the first step on a run-capable
//! host and removed at close; markers, snapshots and the manifest are
//! written as docs/DESIGN.md states; an unchanged fact undoes; a changed
//! one is clobbered under `:clobber` (journaled) and holds the instance
//! under `:defer` (DriftHeld, exit 8, umbras kept, wane not reverting,
//! `--force=drift` reverting); a damaged region is restored whole unless
//! a sibling instance holds a region on the file; a `do` that touches a
//! fact outside its footprint is R0201 and reverts; an unbootstrapped host
//! is R0407; a `:target` undo on a host with no filesystem is R0408; a
//! staged file is removed after its step.

mod common;

use std::collections::BTreeMap;

use common::world::{self, World, OWNER};
use rue_core::body::{lit, FactRef, Prim, RegionSet, Write};
use rue_core::journal::Event as J;
use rue_core::model::{Drift, FootprintEntry, ForceName, Kind, Op, Undo, UndoLocus};
use rue_core::states::State;
use rue_engine::executor::{RPrim, Resolved};
use rue_engine::lifecycle::ApplyOptions;

fn opts() -> ApplyOptions {
    ApplyOptions {
        by: "ops".into(),
        ..ApplyOptions::default()
    }
}

fn write(shape: &str, content: &str) -> Prim {
    Prim::Write(Write {
        fact: FactRef {
            shape: shape.into(),
            anchor: None,
        },
        content: lit(content),
    })
}

fn region_set(shape: &str, anchor: &str, content: &str) -> Prim {
    Prim::RegionSet(RegionSet {
        fact: FactRef {
            shape: shape.into(),
            anchor: Some(anchor.into()),
        },
        content: lit(content),
    })
}

/// An op that owns `file:/own`, modifies `file:/conf` and holds a region
/// on `file:/shared`, all restored by footprint.
fn triple(drift: Option<Drift>) -> Op {
    let mut o = Op::new(
        "cfg",
        vec![
            FootprintEntry::entry(Kind::Owned, "file:/own"),
            FootprintEntry::anchored("file:/shared", "blk"),
            FootprintEntry::entry(Kind::Modified, "file:/conf"),
        ],
    );
    o.undo = Undo::Restore;
    o.undo_locus = UndoLocus::Target;
    o.drift = drift;
    o.do_ = vec![
        write("file:/own", "hello"),
        region_set("file:/shared", "blk", "inside"),
        write("file:/conf", "k=2\n"),
    ];
    o
}

fn seed(w: &World) {
    w.ssh.with(|f| {
        f.facts.insert("file:/conf".into(), b"k=1\n".to_vec());
        f.facts.insert("file:/shared".into(), b"top\n".to_vec());
    });
}

fn fact(w: &World, shape: &str) -> Option<String> {
    w.ssh
        .with(|f| f.facts.get(shape).cloned())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn dir_file(w: &World, id: &str, rel: &str) -> Option<String> {
    w.ssh
        .with(|f| {
            f.files
                .get(&(OWNER.to_string(), id.to_string(), rel.to_string()))
                .cloned()
        })
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

#[test]
fn the_instance_directory_holds_markers_snapshots_and_the_manifest_and_goes_at_close() {
    let mut w = World::new("drift-dir");
    seed(&w);
    let plan = world::temp_plan("p", vec![world::step(triple(None))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let id = out.id.clone();
    assert!(w
        .ssh
        .with(|f| f.dirs.contains(&(OWNER.to_string(), id.clone()))));
    let markers = dir_file(&w, &id, "markers/1").unwrap();
    let lines: Vec<&str> = markers.lines().collect();
    assert_eq!(lines.len(), 3, "{markers}");
    assert!(
        lines[0].starts_with("owned /own ")
            && lines[1].starts_with("region /shared ")
            && lines[2].starts_with("modified /conf "),
        "{markers}"
    );
    assert_eq!(dir_file(&w, &id, "snapshots/1/1").as_deref(), Some("top\n"));
    assert_eq!(dir_file(&w, &id, "snapshots/1/2").as_deref(), Some("k=1\n"));
    assert_eq!(
        dir_file(&w, &id, "manifest").as_deref(),
        Some("region /shared blk\n")
    );
    let rec = w.engine.status(&id).unwrap().unwrap();
    assert_eq!(rec.dirs, vec![OWNER.to_string()]);
    assert_eq!(rec.markers["1"].len(), 3);
    // Unchanged: the undo runs as planned, the marker goes, the directory goes.
    let out = w.engine.recant(&id, &[]).unwrap();
    assert_eq!(out.state, State::Closed);
    assert_eq!(fact(&w, "file:/own"), None);
    assert_eq!(fact(&w, "file:/conf").as_deref(), Some("k=1\n"));
    assert_eq!(fact(&w, "file:/shared").as_deref(), Some("top\n"));
    assert!(!w
        .ssh
        .with(|f| f.dirs.contains(&(OWNER.to_string(), id.clone()))));
    assert!(!w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::DriftClobbered { .. } | J::DriftHeld { .. })));
}

#[test]
fn a_changed_fact_is_clobbered_under_clobber_and_journaled() {
    let mut w = World::new("drift-clobber");
    seed(&w);
    let plan = world::temp_plan("p", vec![world::step(triple(Some(Drift::Clobber)))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    // Another actor edits the modified file and the owned file.
    w.ssh.with(|f| {
        f.facts
            .insert("file:/conf".into(), b"k=3 edited\n".to_vec());
        f.facts.insert("file:/own".into(), b"stranger".to_vec());
    });
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert_eq!(
        fact(&w, "file:/conf").as_deref(),
        Some("k=1\n"),
        "restored anyway"
    );
    assert_eq!(fact(&w, "file:/own"), None, "removed regardless of content");
    assert!(w.sink.events().iter().any(|e| matches!(e, J::DriftClobbered { step: 1, facts } if facts.contains(&"file:/conf".to_string()) && facts.contains(&"file:/own".to_string()))));
}

#[test]
fn a_changed_fact_under_defer_holds_the_instance_until_forced() {
    let mut w = World::new("drift-defer");
    seed(&w);
    let plan = world::temp_plan("p", vec![world::step(triple(Some(Drift::Defer)))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    w.ssh.with(|f| {
        f.facts
            .insert("file:/conf".into(), b"k=3 edited\n".to_vec());
    });
    let out = w.engine.recant(&id, &[]).unwrap();
    assert_eq!((out.state, out.exit), (State::DriftHeld, 8), "{}", out.line);
    assert_eq!(
        fact(&w, "file:/conf").as_deref(),
        Some("k=3 edited\n"),
        "left alone"
    );
    assert_eq!(
        fact(&w, "file:/own").as_deref(),
        Some("hello"),
        "nothing of the step undone"
    );
    assert!(w.sink.events().iter().any(
        |e| matches!(e, J::DriftHeld { step: 1, facts } if facts == &vec!["file:/conf".to_string()])
    ));
    let rec = w.engine.status(&id).unwrap().unwrap();
    assert_eq!(rec.drift_held, vec![1]);
    assert_eq!(
        w.engine.store().read_ledger().unwrap().holdings().len(),
        1,
        "umbras kept"
    );
    // Wane elapsing leaves it; the notification is re-sent.
    w.advance(3600);
    let r = w.engine.reap().unwrap();
    assert_eq!(r.notify, vec![(id.clone(), State::DriftHeld)]);
    assert_eq!(
        w.engine.status(&id).unwrap().unwrap().state,
        State::DriftHeld
    );
    // A plain recant is R0103; --force=drift reverts, clobbering.
    let err = w.engine.recant(&id, &[]).unwrap_err();
    assert!(err.to_string().contains("R0103"), "{err}");
    let out = w.engine.recant(&id, &[ForceName::Drift]).unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert_eq!(fact(&w, "file:/conf").as_deref(), Some("k=1\n"));
    assert_eq!(fact(&w, "file:/own"), None);
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::DriftClobbered { step: 1, .. })));
    assert!(w
        .engine
        .store()
        .read_ledger()
        .unwrap()
        .holdings()
        .is_empty());
}

#[test]
fn a_damaged_region_is_restored_whole_unless_a_sibling_holds_a_region_on_the_file() {
    let mut w = World::new("drift-region");
    seed(&w);
    let plan = world::temp_plan("p", vec![world::step(triple(Some(Drift::Clobber)))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    // The end marker is lost.
    w.ssh.with(|f| {
        f.facts.insert(
            "file:/shared".into(),
            b"top\n# rue-region blk begin\ninside\nedited\n".to_vec(),
        );
    });
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert_eq!(
        fact(&w, "file:/shared").as_deref(),
        Some("top\n"),
        "restored whole from the snapshot"
    );
    assert!(w.sink.events().iter().any(|e| matches!(e, J::DriftClobbered { step: 1, facts } if facts.contains(&"file:/shared".to_string()))));

    // With a sibling instance holding a region on the same file, the
    // damaged region defers (the foreign-region condition).
    let mut w2 = World::new("drift-foreign");
    seed(&w2);
    let mut sibling = Op::new(
        "sib",
        vec![FootprintEntry::anchored("file:/shared", "other")],
    );
    sibling.undo = Undo::Restore;
    sibling.do_ = vec![region_set("file:/shared", "other", "theirs")];
    let s = w2
        .engine
        .apply(
            world::ir(world::temp_plan("s", vec![world::step(sibling)])),
            BTreeMap::new(),
            opts(),
        )
        .unwrap();
    assert_eq!(s.state, State::Applied, "{}", s.line);
    let mine = w2
        .engine
        .apply(
            world::ir(world::temp_plan(
                "p",
                vec![world::step(triple(Some(Drift::Clobber)))],
            )),
            BTreeMap::new(),
            opts(),
        )
        .unwrap();
    assert_eq!(mine.state, State::Applied, "{}", mine.line);
    w2.ssh.with(|f| {
        let text = String::from_utf8_lossy(f.facts.get("file:/shared").unwrap())
            .replace("# rue-region blk end\n", "");
        f.facts.insert("file:/shared".into(), text.into_bytes());
    });
    let out = w2.engine.recant(&mine.id, &[]).unwrap();
    assert_eq!((out.state, out.exit), (State::DriftHeld, 8), "{}", out.line);
    assert!(
        fact(&w2, "file:/shared").unwrap().contains("theirs"),
        "the sibling's region survives"
    );
    // Once the sibling is gone, the forced revert restores whole.
    w2.engine.recant(&s.id, &[]).unwrap();
    let out = w2.engine.recant(&mine.id, &[ForceName::Drift]).unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
}

#[test]
fn a_do_that_touches_another_op_s_fact_is_r0201_and_reverts() {
    let mut w = World::new("drift-r0201");
    seed(&w);
    let mut a = world::op("a");
    a.footprint = vec![FootprintEntry::entry(Kind::Owned, "file:/a")];
    a.undo = Undo::Restore;
    a.do_ = vec![write("file:/a", "mine"), write("file:/conf", "not mine")];
    let mut b = Op::new(
        "b",
        vec![FootprintEntry::entry(Kind::Modified, "file:/conf")],
    );
    b.undo = Undo::Restore;
    b.do_ = vec![write("file:/conf", "k=2\n")];
    let plan = world::temp_plan("p", vec![world::step(a), world::step(b)]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!((out.state, out.exit), (State::Closed, 1), "{}", out.line);
    assert!(w.sink.events().iter().any(|e| matches!(e, J::FootprintViolation { step: 1, facts } if facts == &vec!["file:/conf".to_string()])));
    assert_eq!(fact(&w, "file:/a"), None, "the offending step was undone");
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert!(rec.closed_reason.unwrap().contains("R0201"));
    assert!(
        !w.commands().iter().any(|c| c.contains("do b")),
        "step 2 never ran"
    );
}

#[test]
fn an_unbootstrapped_host_is_r0407_and_a_target_undo_without_a_filesystem_is_r0408() {
    let mut w = World::new("drift-bootstrap");
    seed(&w);
    w.ssh.with(|f| f.bootstrap.instances_dir = false);
    let plan = world::temp_plan("p", vec![world::step(triple(None))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert!(w.sink.events().iter().any(|e| matches!(e, J::Refused { reason } if reason.contains("R0407") && reason.contains("rue bootstrap h"))));
    assert_eq!(fact(&w, "file:/own"), None, "nothing ran");

    let mut w2 = World::new("drift-nofs");
    w2.ssh.with(|f| f.caps.filesystem = false);
    let plan = world::temp_plan("q", vec![world::step(triple(None))]);
    let out = w2
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert!(w2
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::Refused { reason } if reason.contains("R0408"))));
    // A controller-side undo on the same host is fine: no directory, markers
    // on the controller.
    let mut o = triple(None);
    o.undo_locus = UndoLocus::Controller;
    let plan = world::temp_plan("r", vec![world::step(o)]);
    let out = w2
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let rec = w2.engine.status(&out.id).unwrap().unwrap();
    assert!(rec.dirs.is_empty());
    assert_eq!(rec.markers["1"].len(), 3, "markers on the controller");
    let out = w2.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(out.state, State::Closed);
}

#[test]
fn a_staged_file_is_removed_after_its_step_and_journaled() {
    let mut w = World::new("drift-staged");
    let mut o = world::op("a");
    o.do_ = vec![
        Prim::Stage(rue_core::body::Stage {
            name: "creds".into(),
            content: lit("s"),
            mode: 0o600,
        }),
        Prim::Run(rue_core::body::Run {
            cmd: vec![rue_core::body::Part::Lit("use creds".into())],
            env: vec![],
            stdin: None,
        }),
    ];
    let plan = world::temp_plan("p", vec![world::step(o)]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let body = &w.ssh.calls()[0].body;
    assert!(matches!(&body[0], RPrim::Stage { name, mode: 0o600, .. } if name == "creds"));
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::StagedRemoved { step: 1, reason } if reason == "step done")));
    let rec = w.engine.status(&out.id).unwrap().unwrap();
    assert!(rec.staged.is_empty());
    let _ = Resolved::plain("");
}

#[test]
fn a_region_undo_holds_the_host_lock_from_decision_to_write_and_the_manifest_is_never_written_in_place(
) {
    let mut w = World::new("drift-lock");
    seed(&w);
    let plan = world::temp_plan("p", vec![world::step(triple(None))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let ev = w.ssh.events();
    // The manifest goes through replace (write-then-rename), the markers
    // through put, and the do's run precedes both.
    assert!(ev.iter().any(|e| e == "replace manifest"), "{ev:?}");
    assert!(!ev.iter().any(|e| e == "put manifest"), "{ev:?}");
    assert!(ev.iter().position(|e| e == "run") < ev.iter().position(|e| e == "put markers/1"));
    let before_undo = w.ssh.events().len();
    w.engine.recant(&out.id, &[]).unwrap();
    let ev = w.ssh.events();
    // Every read the decision makes is under the lock too: the lock comes
    // first of everything the undo does on the host.
    assert_eq!(
        ev.get(before_undo).map(String::as_str),
        Some("lock"),
        "the lock is the undo's first act on the host, before any read: {ev:?}"
    );
    // The undo's run sits between the lock and its release.
    let lock = ev
        .iter()
        .rposition(|e| e == "lock")
        .expect("a lock was taken");
    let unlock = ev
        .iter()
        .rposition(|e| e == "unlock")
        .expect("and released");
    let run = ev.iter().rposition(|e| e == "run").expect("the undo ran");
    assert!(lock < run && run < unlock, "{ev:?}");
    // The marker was removed after the undo, under the lock too.
    let marker_gone = ev.iter().rposition(|e| e == "remove markers/1").unwrap();
    assert!(run < marker_gone && marker_gone < unlock, "{ev:?}");
}

#[test]
fn staged_files_of_an_instance_not_applying_are_removed_at_boot() {
    let mut w = World::new("drift-staged-boot");
    let plan = world::temp_plan("p", vec![world::step(world::op("a"))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    // A crash left a staged file recorded and the instance resting.
    let mut rec = w.engine.status(&out.id).unwrap().unwrap();
    rec.staged.push(rue_engine::lifecycle::Staged {
        host: OWNER.into(),
        step: 1,
        name: "creds".into(),
    });
    w.engine.store().write_instance(&rec.id, &rec).unwrap();
    w.ssh.with(|f| {
        f.files.insert(
            (OWNER.into(), out.id.clone(), "creds".into()),
            b"s".to_vec(),
        );
    });
    let mut w = w.restart();
    w.engine.boot().unwrap();
    assert!(w
        .sink
        .events()
        .iter()
        .any(|e| matches!(e, J::StagedRemoved { step: 1, reason } if reason == "boot recovery")));
    assert!(w.engine.status(&out.id).unwrap().unwrap().staged.is_empty());
    assert!(w.ssh.events().iter().any(|e| e == "remove creds"));
}

#[test]
fn bootstrap_reports_what_a_host_lacks_with_its_family_s_commands_and_runs_nothing() {
    let mut w = World::new("drift-bootstrap-verb");
    w.ssh.with(|f| {
        f.bootstrap.group = false;
        f.bootstrap.lock = false;
    });
    let (state, commands) = w.engine.bootstrap(OWNER).unwrap();
    assert!(!state.ready());
    assert_eq!(
        commands,
        vec![
            "pw groupadd rue".to_string(),
            "install -o root -g rue -m 0664 /dev/null /var/db/rue/lock".to_string(),
        ]
    );
    assert!(w.commands().is_empty(), "bootstrap runs nothing");
    w.ssh.with(|f| {
        f.bootstrap.group = true;
        f.bootstrap.lock = true;
    });
    let (state, commands) = w.engine.bootstrap(OWNER).unwrap();
    assert!(state.ready() && commands.is_empty());
    assert!(w.engine.bootstrap("nowhere").is_err());
    let report = w.engine.doctor().unwrap();
    assert_eq!(report.hosts.len(), 2);
    let far = report.hosts.iter().find(|h| h.name == world::FAR).unwrap();
    assert!(far.executor.is_none() && far.bootstrap.is_none());
    let h = report.hosts.iter().find(|h| h.name == OWNER).unwrap();
    assert_eq!(h.executor.as_deref(), Some("ssh"));
    assert!(h.bootstrap.as_ref().unwrap().ready());
    // `far` is in the inventory with a transport no executor serves: the
    // report shows it and counts it against health.
    assert!(!report.healthy(), "{report:?}");
    assert_eq!(report.sinks, vec!["mem".to_string()]);
}

#[test]
fn a_fact_above_the_snapshot_cap_refuses_the_step_that_would_snapshot_it() {
    // R0204: the cap the verdict states (`snapshot_cap`) is the cap the
    // engine keeps. A step whose `modified` fact is larger than it is
    // refused before `do`, rather than applied with an undo that cannot
    // restore anything.
    let mut w = World::new("drift-cap");
    let big = vec![b'x'; (rue_engine::lifecycle::SNAPSHOT_CAP + 1) as usize];
    w.ssh.with(|f| {
        f.facts.insert("file:/conf".into(), big);
        f.facts.insert("file:/shared".into(), b"top\n".to_vec());
    });
    let plan = world::temp_plan("p", vec![world::step(triple(None))]);
    let err = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("R0204"), "{err}");
    assert!(err.contains("above the"), "{err}");
    // Nothing ran: the refusal is before the step's `do`.
    assert!(w.commands().is_empty(), "{:?}", w.commands());
}

#[test]
fn a_drift_held_instance_says_r0202_in_its_line() {
    // R0202 is informational: the drift was observed and the step's
    // policy applied to it. The line an operator sees names it.
    let mut w = World::new("drift-r0202");
    seed(&w);
    let plan = world::temp_plan("p", vec![world::step(triple(Some(Drift::Defer)))]);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    w.ssh.with(|f| {
        f.facts.insert("file:/conf".into(), b"edited\n".to_vec());
    });
    let out = w.engine.recant(&out.id, &[]).unwrap();
    assert_eq!(
        out.state,
        rue_core::states::State::DriftHeld,
        "{}",
        out.line
    );
    assert!(out.line.contains("R0202"), "{}", out.line);
    assert_eq!(out.exit, 8);
}
