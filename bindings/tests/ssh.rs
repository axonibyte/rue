//! `ssh()` over the fake transport: what the client is told to run. Every
//! script is one `sh` on the target reading its text from stdin; a run's
//! env and stdin are octal-decoded inside the script, never on a command
//! line; the artifact's helpers are carried so regions follow one rule;
//! outputs come back from stdout; a probe's status is the answer; the lock
//! holder is the host family's tool; the client's own connection failure is
//! Unreachable.

use rue_bindings::ssh::{octal, Exit, FakeTransportHandle, OpenSsh, SshExecutor, Transport};
use rue_core::model::{HostRecord, Tri};
use rue_engine::executor::{ExecError, Executor, ProbeRun, RPrim, Resolved};
use rue_engine::host::Host;

fn host(os: &str) -> Host {
    Host {
        record: HostRecord {
            name: "fw-01".into(),
            os: os.into(),
            reach: vec!["ssh".into()],
            filesystem: true,
            stdin_preamble: true,
            artifact: None,
        },
        address: "10.0.1.1".into(),
        scheduler: Some("cron".into()),
        rue_root: None,
        facts: Default::default(),
    }
}

fn executor() -> (SshExecutor, FakeTransportHandle) {
    let t = FakeTransportHandle::default();
    (SshExecutor::new(Box::new(t.clone())), t)
}

#[test]
fn a_run_s_env_and_stdin_travel_inside_the_script_decoded_by_printf_never_on_a_command_line() {
    let (mut x, t) = executor();
    t.reply(0, "rue-output token=t-1\nhello\n");
    let body = vec![RPrim::Run {
        cmd: Resolved::plain("service sshd reload"),
        env: vec![(
            "PW".into(),
            Resolved {
                text: "s3cr3t".into(),
                secret: true,
            },
        )],
        stdin: Some(Resolved::plain("line\n")),
    }];
    let out = x.run(&host("freebsd"), "i-1", &body).unwrap();
    assert_eq!(out.outputs.get("token").map(String::as_str), Some("t-1"));
    assert_eq!(out.stdout, "hello\n");
    let scripts = t.scripts();
    assert_eq!(scripts.len(), 1);
    let s = &scripts[0];
    assert!(s.starts_with("set -e\n"), "{s}");
    assert!(
        s.contains("ROOT=\"$(printf '%b' '"),
        "the root is decoded: {s}"
    );
    assert!(s.contains("INST=\"$ROOT/instances/i-1\""), "{s}");
    assert!(
        s.contains("strip_region()") && s.contains("region_set()"),
        "the artifact's helpers: {s}"
    );
    assert!(!s.contains("s3cr3t"), "a secret never appears bare: {s}");
    assert!(
        s.contains(&format!("PW=\"$(printf '%b' '{}')\"", octal("s3cr3t"))),
        "{s}"
    );
    assert!(
        s.contains(&format!("printf '%b' '{}' | ", octal("line\n"))),
        "{s}"
    );
    assert!(
        s.contains(&format!(
            "sh -c \"$(printf '%b' '{}')\"",
            octal("service sshd reload")
        )),
        "{s}"
    );
    // A failing script: its status and last stderr line.
    t.with(|f| {
        f.replies.push_back(Exit {
            code: 4,
            stdout: String::new(),
            stderr: "no such service\n".into(),
        })
    });
    let err = x.run(&host("freebsd"), "i-1", &body).unwrap_err();
    assert!(
        matches!(&err, ExecError::Failed(m) if m.contains("exit 4") && m.contains("no such service")),
        "{err}"
    );
}

#[test]
fn file_primitives_become_the_helpers_and_atomic_writes() {
    let (mut x, t) = executor();
    let body = vec![
        RPrim::Write {
            shape: "file:/etc/pf.conf".into(),
            content: Resolved::plain("pass all\n"),
        },
        RPrim::RegionSet {
            shape: "file:/etc/pf.conf".into(),
            anchor: Some("rue-mgmt".into()),
            content: Resolved::plain("pass in proto tcp to port 8443"),
        },
        RPrim::RegionClear {
            shape: "file:/etc/pf.conf".into(),
            anchor: Some("rue-mgmt".into()),
        },
        RPrim::Append {
            shape: "file:/var/log/x".into(),
            line: Resolved::plain("done"),
        },
        RPrim::Remove {
            shape: "file:/tmp/t".into(),
        },
        RPrim::Stage {
            name: "shim".into(),
            content: Resolved::plain("#!/bin/sh\n"),
            mode: 0o700,
        },
    ];
    x.run(&host("linux"), "i-2", &body).unwrap();
    let s = t.scripts().pop().unwrap();
    assert!(
        s.contains("> '/etc/pf.conf'.rue-tmp && mv '/etc/pf.conf'.rue-tmp '/etc/pf.conf'"),
        "{s}"
    );
    assert!(
        s.contains("region_set '/etc/pf.conf' 'rue-mgmt' \"$(printf '%b' '"),
        "{s}"
    );
    assert!(
        s.contains("strip_region '/etc/pf.conf' 'rue-mgmt'\n"),
        "{s}"
    );
    assert!(
        s.contains("printf '%b\\n' '") && s.contains(">> '/var/log/x'"),
        "{s}"
    );
    assert!(s.contains("rm -f '/tmp/t'\n"), "{s}");
    assert!(s.contains("chmod 700 \"$INST\"/'shim'"), "{s}");
    // A non-file fact and a hook primitive are unsupported.
    assert!(x
        .run(
            &host("linux"),
            "i",
            &[RPrim::Remove {
                shape: "proc:x".into()
            }]
        )
        .is_err());
    assert!(x
        .run(
            &host("linux"),
            "i",
            &[RPrim::Hook {
                name: "h".into(),
                args: vec![]
            }]
        )
        .is_err());
}

#[test]
fn a_probe_answers_by_status_and_the_lock_holder_is_the_family_s_tool() {
    let (mut x, t) = executor();
    let probe = ProbeRun {
        name: "reach".into(),
        body: vec![RPrim::Run {
            cmd: Resolved::plain("service-state reach"),
            env: vec![],
            stdin: None,
        }],
    };
    t.reply(1, "down\n");
    let o = x.observe(&host("freebsd"), &probe).unwrap();
    assert_eq!((o.as_tri(), o.text.as_str()), (Tri::No, "down"));
    let s = t.scripts().pop().unwrap();
    assert!(
        !s.contains("set -e"),
        "the probe's own status is the answer: {s}"
    );
    t.reply(0, "up\n");
    assert_eq!(
        x.observe(&host("freebsd"), &probe).unwrap().as_tri(),
        Tri::Yes
    );
    t.reply(9, "");
    assert_eq!(
        x.observe(&host("freebsd"), &probe).unwrap().as_tri(),
        Tri::Unknown
    );
    assert!(x
        .observe(
            &host("freebsd"),
            &ProbeRun {
                name: "n".into(),
                body: vec![]
            }
        )
        .is_err());

    x.host_lock(&host("freebsd")).unwrap();
    x.host_lock(&host("linux")).unwrap();
    let holds = t.with(|f| f.holds.clone());
    assert!(
        holds[0]
            .1
            .contains("lockf -k -t 60 '/var/db/rue/lock' sh -c 'echo ready; cat'"),
        "{}",
        holds[0].1
    );
    assert!(
        holds[1]
            .1
            .contains("flock -w 60 '/var/db/rue/lock' sh -c 'echo ready; cat'"),
        "{}",
        holds[1].1
    );
    match x.host_lock(&host("macos")) {
        Err(e) => assert!(e.to_string().contains("no host lock tool"), "{e}"),
        Ok(_) => panic!("a lock tool for macos was invented"),
    }
}

#[test]
fn instance_directory_ops_read_fact_and_bootstrap_state_go_through_scripts() {
    let (mut x, t) = executor();
    let h = host("freebsd");
    x.instance_dir_create(&h, "i-3").unwrap();
    x.put_file(&h, "i-3", "markers/1", b"owned /x abc\n", 0o640)
        .unwrap();
    x.replace_file(&h, "i-3", "deadline", b"9\n").unwrap();
    t.reply(0, "9\n");
    assert_eq!(x.get_file(&h, "i-3", "deadline").unwrap(), b"9\n");
    x.remove_file(&h, "i-3", "markers/1").unwrap();
    t.reply(0, "i-3 1 0 2770\ni-4 0 1 755\n");
    let list = x.instance_dir_list(&h).unwrap();
    assert_eq!(
        (
            list[0].instance.as_str(),
            list[0].armed,
            list[0].fired,
            list[0].modes_ok
        ),
        ("i-3", true, false, true)
    );
    assert_eq!(
        (
            list[1].instance.as_str(),
            list[1].armed,
            list[1].fired,
            list[1].modes_ok
        ),
        ("i-4", false, true, false)
    );
    // The clock probe an arm makes before it writes a deadline (R0403).
    t.reply(0, "1700000000\n");
    assert_eq!(
        x.clock_now(&h).unwrap().map(|i| i.unix_s),
        Some(1_700_000_000)
    );
    t.reply(0, "not a time\n");
    assert!(x.clock_now(&h).is_err());
    t.reply(3, "");
    assert_eq!(x.read_fact(&h, "file:/nope").unwrap(), None);
    t.reply(0, "content");
    assert_eq!(
        x.read_fact(&h, "file:/yes").unwrap(),
        Some(b"content".to_vec())
    );
    t.reply(0, "root=1 group=1 instances=1 lock=1 mi=2770 ml=664\n");
    let st = x.bootstrap_state(&h).unwrap();
    assert!(st.ready(), "{st:?}");
    t.reply(0, "root=1 group=0 instances=1 lock=1 mi=770 ml=664\n");
    let st = x.bootstrap_state(&h).unwrap();
    assert!(!st.group && !st.modes_ok && !st.ready());
    x.instance_dir_remove(&h, "i-3").unwrap();
    let scripts = t.scripts();
    assert!(
        scripts[0]
            .contains("mkdir -p \"$INST/markers\" \"$INST/snapshots\" && chmod 2770 \"$INST\""),
        "{}",
        scripts[0]
    );
    assert!(
        scripts[1].contains("chmod 640 \"$INST\"/'markers/1'.rue-tmp && mv"),
        "{}",
        scripts[1]
    );
    assert!(scripts.last().unwrap().contains("rm -rf \"$INST\""));
}

#[test]
fn the_openssh_client_reads_nothing_of_the_user_s_and_reports_its_own_failure_as_unreachable() {
    // A transport pointed at nothing: the client's exit 255 is Unreachable.
    let mut t = OpenSsh {
        identity: "/nonexistent/key".into(),
        known_hosts: "/nonexistent/known_hosts".into(),
        user: "nobody".into(),
        connect_timeout: 1,
    };
    let mut h = host("freebsd");
    h.address = "127.0.0.1".into();
    h.record.name = "loop".into();
    // Port 22 may or may not answer here; either way the client must fail
    // before any command runs, with no key and no known host, and say so.
    match t.exec(&h, "echo hi\n") {
        Err(ExecError::Unreachable(m)) => assert!(m.contains("127.0.0.1"), "{m}"),
        Err(ExecError::Io(m)) => assert!(m.contains("ssh"), "{m}"),
        other => panic!("ssh with no key and no known host answered {other:?}"),
    }
}
