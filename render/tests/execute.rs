//! The artifacts run. Each scenario builds a temporary instance directory
//! (markers with real digests, snapshots, deadline and heartbeat files, a
//! sibling manifest) and a temporary world of files, renders the plan for a
//! host whose `rue_root` is the temporary root, executes the artifact the
//! way the scheduler will (`sh artifact.sh`; `uv run --offline --script
//! artifact.py`), and reads the world back. Every scenario runs in both
//! languages, so the two templates are held to one behavior.
//!
//! `#[cfg(unix)]`: the artifacts executed here are the POSIX ones, and the
//! windows-gnu suite under wine has neither `sh` nor `uv`. PowerShell is not
//! executed anywhere this unit. `sh` and `uv` are required, not optional: a
//! gate host without them fails here.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rue_core::body::*;
use rue_core::model::*;
use rue_render::{render, Bindings, Instance};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lang {
    Sh,
    Python,
}

const LANGS: [Lang; 2] = [Lang::Sh, Lang::Python];

/// One scenario's world: a root for the instance directory and a directory
/// of facts, both under a fresh temporary directory.
struct World {
    dir: PathBuf,
    inst: PathBuf,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl World {
    fn new(name: &str) -> World {
        let dir = std::env::temp_dir().join(format!(
            "rue-execute-{}-{}-{}",
            name,
            std::process::id(),
            now()
        ));
        let _ = fs::remove_dir_all(&dir);
        let inst = dir.join("root").join("instances").join("i-1");
        fs::create_dir_all(inst.join("markers")).unwrap();
        fs::create_dir_all(dir.join("facts")).unwrap();
        World { dir, inst }
    }

    fn root(&self) -> String {
        self.dir.join("root").to_str().unwrap().to_string()
    }

    fn fact(&self, name: &str) -> PathBuf {
        self.dir.join("facts").join(name)
    }

    fn shape(&self, name: &str) -> String {
        format!("file:{}", self.fact(name).display())
    }

    fn put(&self, name: &str, content: &str) {
        fs::write(self.fact(name), content).unwrap();
    }

    fn get(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.fact(name)).ok()
    }

    /// A completion marker for step `n` recording the facts' current hashes.
    fn marker(&self, n: u32, facts: &[(&str, Kind)]) {
        let lines: Vec<String> = facts
            .iter()
            .map(|(name, kind)| {
                let bytes = fs::read(self.fact(name)).unwrap_or_default();
                format!(
                    "{} {} {}",
                    match kind {
                        Kind::Owned => "owned",
                        Kind::Region => "region",
                        Kind::Modified => "modified",
                        _ => unreachable!(),
                    },
                    self.fact(name).display(),
                    sha(&bytes)
                )
            })
            .collect();
        fs::write(
            self.inst.join("markers").join(n.to_string()),
            lines.join("\n") + "\n",
        )
        .unwrap();
    }

    fn snapshot(&self, n: u32, k: usize, content: &str) {
        let d = self.inst.join("snapshots").join(n.to_string());
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(k.to_string()), content).unwrap();
    }

    fn deadline(&self, at: u64) {
        fs::write(self.inst.join("deadline"), format!("{at}\n")).unwrap();
    }

    fn heartbeat(&self, at: u64) {
        fs::write(self.inst.join("heartbeat"), format!("{at}\n")).unwrap();
    }

    fn sibling_manifest(&self, path: &Path, anchor: &str) {
        let d = self.dir.join("root").join("instances").join("other");
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("manifest"),
            format!("region {} {anchor}\n", path.display()),
        )
        .unwrap();
    }

    fn marker_present(&self, n: u32) -> bool {
        self.inst.join("markers").join(n.to_string()).exists()
    }

    fn flag(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.inst.join(name)).ok()
    }

    /// Render the plan for `lang`, write the artifact beside the instance,
    /// run it, and fail on a non-zero exit.
    fn fire(&self, lang: Lang, plan: &Plan) {
        let site = Site {
            hosts: vec![HostRecord {
                name: "h".into(),
                os: "freebsd".into(),
                reach: vec!["ssh".into()],
                filesystem: true,
                stdin_preamble: true,
                artifact: Some(match lang {
                    Lang::Sh => ArtifactLanguage::Sh,
                    Lang::Python => ArtifactLanguage::Python,
                }),
            }],
            transports: vec!["ssh".into()],
            authenticators: vec![],
            max_wait: None,
            scheduler_present: vec!["h".into()],
            secrets_deliver_to: vec![],
        };
        let instance = Instance {
            id: "i-1".into(),
            rue_root: Some(self.root()),
        };
        let a = render(&site, plan, "h", &instance, &Bindings::default()).unwrap();
        let path = self.inst.join(a.file_name);
        fs::write(&path, &a.text).unwrap();
        let out = match lang {
            Lang::Sh => Command::new("sh").arg(&path).output(),
            Lang::Python => Command::new("uv")
                .args(["run", "--offline", "--script"])
                .arg(&path)
                .output(),
        }
        .unwrap_or_else(|e| {
            panic!("{lang:?}: cannot run the artifact: {e} (sh and uv are required)")
        });
        assert!(
            out.status.success(),
            "{lang:?}: artifact exited {:?}\nstdout: {}\nstderr: {}\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            a.text
        );
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn s(o: Op) -> Item {
    Item::Step(StepI::new(o))
}

fn target(o: Op) -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        ..o
    }
}

fn plan(items: Vec<Item>, triggers: Vec<Trigger>) -> Plan {
    Plan {
        wane: Some(Duration::new(3600)),
        backstop: Some(Backstop {
            triggers,
            arm_before: 1,
        }),
        ..Plan::new("p", "h", items)
    }
}

fn after() -> Vec<Trigger> {
    vec![Trigger::After(Duration::new(3600))]
}

const REGION: &str = "keep\n# rue-region blk begin\nours\n# rue-region blk end\ntail\n";

/// The three restore kinds over one plan: an owned file, a fenced region and
/// a modified file with its snapshot, plus a run body (step 4) that copies
/// the owned file into a witness. Reverse order means step 4 runs before
/// step 1 removes that file, so the witness holds its content; forward
/// order would leave the witness empty.
fn three_kinds(w: &World) -> Plan {
    plan(
        vec![
            s(target(Op::new(
                "own",
                vec![FootprintEntry::entry(Kind::Owned, &w.shape("own"))],
            ))),
            s(target(Op::new(
                "reg",
                vec![FootprintEntry::anchored(&w.shape("reg"), "blk")],
            ))),
            s(target(Op::new(
                "mod",
                vec![FootprintEntry::entry(Kind::Modified, &w.shape("mod"))],
            ))),
            s(target(Op {
                undo: Undo::Computed {
                    body: vec![run_lit(&format!(
                        "cat '{}' > '{}' 2>/dev/null || : > '{}'",
                        w.fact("own").display(),
                        w.fact("witness").display(),
                        w.fact("witness").display()
                    ))],
                    undo_pre: vec![w.shape("own")],
                },
                ..Op::new("run", vec![FootprintEntry::entry(Kind::Owned, "svc:x")])
            })),
        ],
        after(),
    )
}

fn seed(w: &World) {
    w.put("own", "owned content\n");
    w.put("reg", REGION);
    w.put("mod", "changed by do\n");
    w.snapshot(2, 0, "keep\ntail\n");
    w.snapshot(3, 0, "original\n");
    w.marker(1, &[("own", Kind::Owned)]);
    w.marker(2, &[("reg", Kind::Region)]);
    w.marker(3, &[("mod", Kind::Modified)]);
    w.marker(4, &[]);
}

#[test]
fn not_yet_due_touches_nothing() {
    for lang in LANGS {
        let w = World::new("notdue");
        seed(&w);
        w.deadline(now() + 3600);
        w.fire(lang, &three_kinds(&w));
        assert_eq!(w.get("own").as_deref(), Some("owned content\n"), "{lang:?}");
        assert_eq!(w.get("reg").as_deref(), Some(REGION), "{lang:?}");
        assert_eq!(w.get("mod").as_deref(), Some("changed by do\n"), "{lang:?}");
        assert!(w.get("witness").is_none(), "{lang:?}");
        assert!((1..=4).all(|n| w.marker_present(n)), "{lang:?}");
        assert!(w.flag("fired").is_none(), "{lang:?}");
    }
}

#[test]
fn due_undoes_every_marked_step_in_reverse_and_fires_once() {
    for lang in LANGS {
        let w = World::new("due");
        seed(&w);
        w.deadline(now() - 1);
        let p = three_kinds(&w);
        w.fire(lang, &p);
        assert!(w.get("own").is_none(), "{lang:?}: owned file removed");
        assert_eq!(
            w.get("reg").as_deref(),
            Some("keep\ntail\n"),
            "{lang:?}: region stripped"
        );
        assert_eq!(
            w.get("mod").as_deref(),
            Some("original\n"),
            "{lang:?}: modified restored"
        );
        assert_eq!(
            w.get("witness").as_deref(),
            Some("owned content\n"),
            "{lang:?}: the run body (step 4) ran before step 1 removed the file"
        );
        assert!(
            (1..=4).all(|n| !w.marker_present(n)),
            "{lang:?}: markers removed"
        );
        assert!(w.flag("fired").is_some(), "{lang:?}");
        assert!(
            w.flag("drift").is_none() && w.flag("clobbered").is_none(),
            "{lang:?}"
        );
        // A fired artifact does nothing on a second run.
        w.put("own", "back\n");
        w.fire(lang, &p);
        assert_eq!(
            w.get("own").as_deref(),
            Some("back\n"),
            "{lang:?}: a fired artifact does nothing"
        );
    }
}

#[test]
fn a_step_without_a_marker_is_skipped() {
    for lang in LANGS {
        let w = World::new("unmarked");
        seed(&w);
        fs::remove_file(w.inst.join("markers").join("1")).unwrap();
        w.deadline(now() - 1);
        w.fire(lang, &three_kinds(&w));
        assert_eq!(
            w.get("own").as_deref(),
            Some("owned content\n"),
            "{lang:?}: unmarked step left alone"
        );
        assert_eq!(w.get("reg").as_deref(), Some("keep\ntail\n"), "{lang:?}");
        assert!(w.flag("fired").is_some(), "{lang:?}");
    }
}

#[test]
fn drift_defer_leaves_a_changed_fact_and_marks_it_and_clobber_restores_and_marks_it() {
    for lang in LANGS {
        for drift in [Drift::Defer, Drift::Clobber] {
            let w = World::new("drift");
            w.put("mod", "changed by do\n");
            w.snapshot(1, 0, "original\n");
            w.marker(1, &[("mod", Kind::Modified)]);
            // Someone edited the fact after do.
            w.put("mod", "edited by a stranger\n");
            w.deadline(now() - 1);
            let p = plan(
                vec![s(target(Op {
                    drift: Some(drift),
                    ..Op::new(
                        "mod",
                        vec![FootprintEntry::entry(Kind::Modified, &w.shape("mod"))],
                    )
                }))],
                after(),
            );
            w.fire(lang, &p);
            match drift {
                Drift::Defer => {
                    assert_eq!(
                        w.get("mod").as_deref(),
                        Some("edited by a stranger\n"),
                        "{lang:?}"
                    );
                    assert_eq!(w.flag("drift").as_deref(), Some("1\n"), "{lang:?}");
                    assert!(
                        w.marker_present(1),
                        "{lang:?}: a deferred step keeps its marker"
                    );
                }
                Drift::Clobber => {
                    assert_eq!(w.get("mod").as_deref(), Some("original\n"), "{lang:?}");
                    assert_eq!(w.flag("clobbered").as_deref(), Some("1\n"), "{lang:?}");
                    assert!(!w.marker_present(1), "{lang:?}");
                }
            }
            assert!(w.flag("fired").is_some(), "{lang:?}");
        }
    }
}

#[test]
fn damaged_region_markers_restore_the_whole_file_unless_a_sibling_holds_a_region() {
    for lang in LANGS {
        for foreign in [false, true] {
            let w = World::new("region");
            w.put("reg", "keep\n# rue-region blk begin\nours\ntail\n"); // end marker lost
            w.snapshot(1, 0, "keep\ntail\n");
            w.marker(1, &[("reg", Kind::Region)]);
            if foreign {
                w.sibling_manifest(&w.fact("reg"), "theirs");
            }
            w.deadline(now() - 1);
            let p = plan(
                vec![s(target(Op::new(
                    "reg",
                    vec![FootprintEntry::anchored(&w.shape("reg"), "blk")],
                )))],
                after(),
            );
            w.fire(lang, &p);
            if foreign {
                assert_eq!(
                    w.get("reg").as_deref(),
                    Some("keep\n# rue-region blk begin\nours\ntail\n"),
                    "{lang:?}: deferred"
                );
                assert_eq!(w.flag("drift").as_deref(), Some("1\n"), "{lang:?}");
            } else {
                assert_eq!(
                    w.get("reg").as_deref(),
                    Some("keep\ntail\n"),
                    "{lang:?}: restored whole"
                );
                assert_eq!(w.flag("clobbered").as_deref(), Some("1\n"), "{lang:?}");
            }
        }
    }
}

#[test]
fn a_stale_heartbeat_fires_and_a_fresh_one_does_not() {
    for lang in LANGS {
        for (age, fires) in [(0u64, false), (120, true)] {
            let w = World::new("heartbeat");
            w.put("own", "x\n");
            w.marker(1, &[("own", Kind::Owned)]);
            w.heartbeat(now() - age);
            let p = plan(
                vec![s(target(Op::new(
                    "own",
                    vec![FootprintEntry::entry(Kind::Owned, &w.shape("own"))],
                )))],
                vec![Trigger::UnlessHeartbeat {
                    deadline: Duration::new(60),
                    interval: Some(Duration::new(20)),
                }],
            );
            w.fire(lang, &p);
            assert_eq!(w.get("own").is_none(), fires, "{lang:?} age {age}");
            assert_eq!(w.flag("fired").is_some(), fires, "{lang:?} age {age}");
        }
        // No heartbeat file at all: the engine never wrote one; fire.
        let w = World::new("noheartbeat");
        w.put("own", "x\n");
        w.marker(1, &[("own", Kind::Owned)]);
        let p = plan(
            vec![s(target(Op::new(
                "own",
                vec![FootprintEntry::entry(Kind::Owned, &w.shape("own"))],
            )))],
            vec![Trigger::UnlessHeartbeat {
                deadline: Duration::new(60),
                interval: None,
            }],
        );
        w.fire(lang, &p);
        assert!(w.get("own").is_none(), "{lang:?}");
    }
}

#[test]
fn a_quoted_value_survives_the_shell_intact() {
    // A run body whose interpolated value carries every character the
    // quoting must defend against, baked through the bindings and written
    // by the shell to a witness file.
    for lang in LANGS {
        let w = World::new("quote");
        w.marker(1, &[]);
        w.deadline(now() - 1);
        let hostile = "it's; $(reboot) `x` \"q\" \\ done";
        let p = plan(
            vec![s(target(Op {
                undo: Undo::Computed {
                    body: vec![run(vec![
                        text("printf %s "),
                        interp(param("v")),
                        text(&format!(" > '{}'", w.fact("witness").display())),
                    ])],
                    undo_pre: vec!["svc:x".into()],
                },
                ..Op::new("q", vec![FootprintEntry::entry(Kind::Owned, "svc:x")])
            }))],
            after(),
        );
        let site = Site {
            hosts: vec![HostRecord {
                name: "h".into(),
                os: "linux".into(),
                reach: vec!["ssh".into()],
                filesystem: true,
                stdin_preamble: true,
                artifact: Some(match lang {
                    Lang::Sh => ArtifactLanguage::Sh,
                    Lang::Python => ArtifactLanguage::Python,
                }),
            }],
            transports: vec!["ssh".into()],
            authenticators: vec![],
            max_wait: None,
            scheduler_present: vec!["h".into()],
            secrets_deliver_to: vec![],
        };
        let mut b = Bindings::default();
        b.params.insert("v".into(), hostile.into());
        let instance = Instance {
            id: "i-1".into(),
            rue_root: Some(w.root()),
        };
        let a = render(&site, &p, "h", &instance, &b).unwrap();
        let path = w.inst.join(a.file_name);
        fs::write(&path, &a.text).unwrap();
        let out = match lang {
            Lang::Sh => Command::new("sh").arg(&path).output().unwrap(),
            Lang::Python => Command::new("uv")
                .args(["run", "--offline", "--script"])
                .arg(&path)
                .output()
                .unwrap(),
        };
        assert!(
            out.status.success(),
            "{lang:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(w.get("witness").as_deref(), Some(hostile), "{lang:?}");
    }
}
