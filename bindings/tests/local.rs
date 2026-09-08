//! `local()` against real files in a temporary root: a run with its env on
//! the child and its stdin piped, outputs read back from stdout, a failing
//! command's status, secrets scrubbed from what is reported, every file
//! primitive, a probe's exit status as the guard's answer, the instance
//! directory ops with their modes, and the host lock.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use rue_bindings::local::LocalExecutor;
use rue_core::model::{HostRecord, Tri};
use rue_engine::executor::{Executor, ProbeRun, RPrim, Resolved};
use rue_engine::host::Host;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "rue-local-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 1_000_000
        ));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn host(root: &std::path::Path) -> Host {
    Host {
        record: HostRecord {
            name: "here".into(),
            os: "freebsd".into(),
            reach: vec!["local".into()],
            filesystem: true,
            stdin_preamble: true,
            artifact: None,
        },
        address: "127.0.0.1".into(),
        scheduler: None,
        rue_root: Some(root.to_string_lossy().into_owned()),
        facts: Default::default(),
    }
}

fn run(cmd: &str) -> RPrim {
    RPrim::Run {
        cmd: Resolved::plain(cmd),
        env: vec![],
        stdin: None,
    }
}

#[test]
fn a_run_gets_its_env_and_stdin_and_reports_outputs_status_and_scrubbed_text() {
    let d = TempDir::new("run");
    let h = host(&d.0);
    let mut x = LocalExecutor::default();
    let body = vec![RPrim::Run {
        cmd: Resolved::plain(
            "read -r line; echo \"got $line and $TOKEN\"; echo 'rue-output name=v1'",
        ),
        env: vec![(
            "TOKEN".into(),
            Resolved {
                text: "hunter2".into(),
                secret: true,
            },
        )],
        stdin: Some(Resolved::plain("in\n")),
    }];
    let out = x.run(&h, "i", &body).unwrap();
    assert_eq!(out.outputs.get("name").map(String::as_str), Some("v1"));
    assert_eq!(
        out.stdout, "got in and <secret>\n",
        "the secret is scrubbed"
    );
    // A failing command: its status and last stderr line, scrubbed.
    let err = x
        .run(
            &h,
            "i",
            &[RPrim::Run {
                cmd: Resolved::plain("echo \"bad $TOKEN\" >&2; exit 3"),
                env: vec![(
                    "TOKEN".into(),
                    Resolved {
                        text: "hunter2".into(),
                        secret: true,
                    },
                )],
                stdin: None,
            }],
        )
        .unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("exit 3") && text.contains("bad <secret>"),
        "{text}"
    );
    assert!(!text.contains("hunter2"));
}

#[test]
fn file_primitives_act_on_real_files_with_regions_by_the_artifact_s_rule() {
    let d = TempDir::new("files");
    let h = host(&d.0);
    let mut x = LocalExecutor::default();
    let f = d.0.join("conf");
    let shape = format!("file:{}", f.display());
    fs::write(&f, "top\n").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o640)).unwrap();
    let body = vec![
        RPrim::Append {
            shape: shape.clone(),
            line: Resolved::plain("k=1"),
        },
        RPrim::RegionSet {
            shape: shape.clone(),
            anchor: Some("blk".into()),
            content: Resolved::plain("inside"),
        },
    ];
    x.run(&h, "i", &body).unwrap();
    assert_eq!(
        fs::read_to_string(&f).unwrap(),
        "top\nk=1\n# rue-region blk begin\ninside\n# rue-region blk end\n"
    );
    assert_eq!(
        fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o640,
        "the mode survives"
    );
    x.run(
        &h,
        "i",
        &[RPrim::RegionClear {
            shape: shape.clone(),
            anchor: Some("blk".into()),
        }],
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), "top\nk=1\n");
    // Damaged markers refuse rather than guess.
    fs::write(&f, "top\n# rue-region blk begin\nx\n").unwrap();
    let err = x
        .run(
            &h,
            "i",
            &[RPrim::RegionClear {
                shape: shape.clone(),
                anchor: Some("blk".into()),
            }],
        )
        .unwrap_err();
    assert!(err.to_string().contains("damaged"), "{err}");
    // Write, read back, remove, remove again.
    let g = d.0.join("new");
    let gs = format!("file:{}", g.display());
    x.run(
        &h,
        "i",
        &[RPrim::Write {
            shape: gs.clone(),
            content: Resolved::plain("hello"),
        }],
    )
    .unwrap();
    assert_eq!(x.read_fact(&h, &gs).unwrap(), Some(b"hello".to_vec()));
    x.run(&h, "i", &[RPrim::Remove { shape: gs.clone() }])
        .unwrap();
    assert_eq!(x.read_fact(&h, &gs).unwrap(), None);
    x.run(&h, "i", &[RPrim::Remove { shape: gs.clone() }])
        .unwrap();
    // A staged file lands in the instance directory with its mode.
    x.run(
        &h,
        "inst-1",
        &[RPrim::Stage {
            name: "secret.txt".into(),
            content: Resolved::plain("s"),
            mode: 0o600,
        }],
    )
    .unwrap();
    let staged = d.0.join("instances/inst-1/secret.txt");
    assert_eq!(fs::read_to_string(&staged).unwrap(), "s");
    assert_eq!(
        fs::metadata(&staged).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // A non-file fact is unsupported, as is a hook primitive.
    assert!(x
        .run(
            &h,
            "i",
            &[RPrim::Remove {
                shape: "proc:x".into()
            }]
        )
        .is_err());
    assert!(x
        .run(
            &h,
            "i",
            &[RPrim::Hook {
                name: "h".into(),
                args: vec![]
            }]
        )
        .is_err());
}

#[test]
fn a_probe_answers_by_exit_status_and_its_stdout_is_the_fact() {
    let d = TempDir::new("probe");
    let h = host(&d.0);
    let mut x = LocalExecutor::default();
    let probe = |cmd: &str| ProbeRun {
        name: "p".into(),
        body: vec![run(cmd)],
    };
    let o = x.observe(&h, &probe("echo up; exit 0")).unwrap();
    assert_eq!((o.as_tri(), o.text.as_str()), (Tri::Yes, "up"));
    assert_eq!(x.observe(&h, &probe("exit 1")).unwrap().as_tri(), Tri::No);
    assert_eq!(
        x.observe(&h, &probe("exit 7")).unwrap().as_tri(),
        Tri::Unknown
    );
    let err = x
        .observe(
            &h,
            &ProbeRun {
                name: "named-only".into(),
                body: vec![],
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("knows no probe by name"), "{err}");
}

#[test]
fn instance_directories_carry_their_modes_and_the_host_lock_holds() {
    let d = TempDir::new("dirs");
    let h = host(&d.0);
    let mut x = LocalExecutor::default();
    let st = x.bootstrap_state(&h).unwrap();
    assert!(st.rue_root && !st.instances_dir && !st.lock && !st.modes_ok);
    // Bootstrap by hand, as the operator would.
    fs::create_dir_all(d.0.join("instances")).unwrap();
    fs::set_permissions(d.0.join("instances"), fs::Permissions::from_mode(0o2770)).unwrap();
    fs::write(d.0.join("lock"), "").unwrap();
    fs::set_permissions(d.0.join("lock"), fs::Permissions::from_mode(0o664)).unwrap();
    let st = x.bootstrap_state(&h).unwrap();
    assert!(
        st.rue_root && st.instances_dir && st.lock && st.modes_ok,
        "{st:?}"
    );
    x.instance_dir_create(&h, "i-1").unwrap();
    let dir = d.0.join("instances/i-1");
    assert!(dir.join("markers").is_dir() && dir.join("snapshots").is_dir());
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o7777,
        0o2770
    );
    x.put_file(&h, "i-1", "markers/1", b"owned /x abc\n", 0o640)
        .unwrap();
    assert_eq!(
        fs::metadata(dir.join("markers/1"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    x.replace_file(&h, "i-1", "deadline", b"123\n").unwrap();
    assert_eq!(x.get_file(&h, "i-1", "deadline").unwrap(), b"123\n");
    assert!(!dir.join("deadline.rue-tmp").exists());
    let list = x.instance_dir_list(&h).unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].armed && !list[0].fired);
    fs::write(dir.join("fired"), "").unwrap();
    let list = x.instance_dir_list(&h).unwrap();
    assert!(!list[0].armed && list[0].fired);
    // The lock: held while the guard lives; a second taker waits.
    let guard = x.host_lock(&h).unwrap();
    let lock_path = d.0.join("lock");
    let contended = std::thread::spawn(move || {
        let f = fs::File::open(&lock_path).unwrap();
        use std::os::unix::io::AsRawFd;
        unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }
    })
    .join()
    .unwrap();
    assert_ne!(contended, 0, "the lock was free while the guard lived");
    drop(guard);
    // Released on drop. A test running beside this one may have forked a
    // child that inherited the descriptor for the instant before its exec
    // closes it, so the release is awaited briefly rather than asserted at
    // once.
    let f = fs::File::open(d.0.join("lock")).unwrap();
    use std::os::unix::io::AsRawFd;
    let mut rc = -1;
    for _ in 0..100 {
        rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        rc,
        0,
        "released on drop: {}",
        std::io::Error::last_os_error()
    );
    x.instance_dir_remove(&h, "i-1").unwrap();
    assert!(!dir.exists());
    x.instance_dir_remove(&h, "i-1").unwrap();
}
