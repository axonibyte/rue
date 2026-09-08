//! Tier 1 for the renderer: the covered set and its order, each trigger's
//! presence, the refusals, and that nothing a target-side artifact cannot
//! carry ever reaches its text.

use rue_core::backstop::coverage;
use rue_core::body::*;
use rue_core::diagnostics::Code;
use rue_core::model::*;
use rue_render::{render, Artifact, Bindings, Instance, RenderError};

fn host(name: &str, os: &str, artifact: Option<ArtifactLanguage>) -> HostRecord {
    HostRecord {
        name: name.into(),
        os: os.into(),
        reach: vec!["ssh".into()],
        filesystem: true,
        stdin_preamble: true,
        artifact,
    }
}

fn site() -> Site {
    Site {
        hosts: vec![
            host("fw", "freebsd", None),
            host("win", "windows", None),
            host("mac", "macos", None),
            host("py", "linux", Some(ArtifactLanguage::Python)),
            host("wpy", "windows", Some(ArtifactLanguage::Python)),
            host("bad", "windows", Some(ArtifactLanguage::Sh)),
        ],
        transports: vec!["ssh".into()],
        authenticators: vec![],
        max_wait: None,
        scheduler_present: vec![],
        secrets_deliver_to: vec![],
    }
}

fn owned(f: &str) -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        do_: vec![write(fact_ref(&format!("file:/etc/{f}")), lit("x"))],
        ..Op::new(
            f,
            vec![FootprintEntry::entry(
                Kind::Owned,
                &format!("file:/etc/{f}"),
            )],
        )
    }
}

fn s(o: Op) -> Item {
    Item::Step(StepI::new(o))
}

fn plan(owner: &str, items: Vec<Item>, triggers: Vec<Trigger>) -> Plan {
    Plan {
        wane: Some(Duration::new(3600)),
        backstop: Some(Backstop {
            triggers,
            arm_before: 1,
        }),
        ..Plan::new("p", owner, items)
    }
}

fn after() -> Vec<Trigger> {
    vec![Trigger::After(Duration::new(3600))]
}

fn inst() -> Instance {
    Instance {
        id: "i-1".into(),
        rue_root: None,
    }
}

fn render_on(host: &str, p: &Plan) -> Result<Artifact, RenderError> {
    render(&site(), p, host, &inst(), &Bindings::default())
}

fn step_order(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|l| l.strip_prefix("# step "))
        .map(|l| l.split(':').next().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn the_covered_steps_appear_in_reverse_order_and_nothing_else_does() {
    let p = plan(
        "fw",
        vec![
            s(owned("a")),
            s(Op {
                undo_locus: UndoLocus::Controller,
                ..owned("b")
            }),
            s(owned("c")),
            Item::Commit,
        ],
        after(),
    );
    let cov = coverage(&p).unwrap();
    assert_eq!(cov.covered, vec![1, 3]);
    for h in ["fw", "mac", "py"] {
        let a = render_on(h, &p).unwrap();
        assert_eq!(step_order(&a.text), vec![3, 1], "{h}");
        assert!(
            a.text.contains("rm -f '/etc/a'") || a.text.contains("remove('/etc/a')"),
            "{h}"
        );
        assert!(
            !a.text.contains("/etc/b"),
            "{h}: an uncovered step's undo was rendered"
        );
    }
}

#[test]
fn each_language_has_its_file_name_header_and_shell_launcher() {
    let p = plan("fw", vec![s(owned("a"))], after());
    let sh = render_on("fw", &p).unwrap();
    assert_eq!(
        (sh.language, sh.file_name),
        (ArtifactLanguage::Sh, "artifact.sh")
    );
    assert!(sh.text.starts_with(
        "#!/bin/sh\n# rue backstop artifact: plan p on fw (os freebsd), instance i-1, language sh."
    ));
    assert!(sh.text.contains("shasum -a 256"));
    let mac = render_on("mac", &plan("mac", vec![s(owned("a"))], after())).unwrap();
    assert_eq!(mac.language, ArtifactLanguage::Sh);
    assert!(mac.text.contains("(os macos)"));

    let ps = render_on("win", &plan("win", vec![s(owned("a"))], after())).unwrap();
    assert_eq!(
        (ps.language, ps.file_name),
        (ArtifactLanguage::Powershell, "artifact.ps1")
    );
    assert!(ps.text.contains("Set-StrictMode"));
    assert!(ps.text.contains("$Root = 'C:\\ProgramData\\rue'"));

    let py = render_on("py", &plan("py", vec![s(owned("a"))], after())).unwrap();
    assert_eq!(
        (py.language, py.file_name),
        (ArtifactLanguage::Python, "artifact.py")
    );
    assert!(py.text.starts_with("#!/usr/bin/env -S uv run --script\n# /// script\n# requires-python = \">=3.11\"\n# dependencies = []\n# ///\n"));
    assert!(py.text.contains("RUN = ['sh', '-c']"));
    assert!(py.text.contains("ROOT = '/var/db/rue'"));
    let wpy = render_on("wpy", &plan("wpy", vec![s(owned("a"))], after())).unwrap();
    assert!(wpy
        .text
        .contains("RUN = ['powershell.exe', '-NoProfile', '-NonInteractive', '-Command']"));
    assert!(wpy.text.contains("ROOT = 'C:\\\\ProgramData\\\\rue'"));
}

#[test]
fn the_rue_root_can_be_overridden() {
    let p = plan("fw", vec![s(owned("a"))], after());
    let i = Instance {
        id: "x".into(),
        rue_root: Some("/tmp/r".into()),
    };
    let a = render(&site(), &p, "fw", &i, &Bindings::default()).unwrap();
    assert!(a.text.contains("INST='/tmp/r/instances/x'"));
}

#[test]
fn triggers_select_the_deadline_and_heartbeat_tests() {
    let with = |t: Vec<Trigger>| {
        render_on("fw", &plan("fw", vec![s(owned("a"))], t))
            .unwrap()
            .text
    };
    let a = with(after());
    assert!(a.contains("$INST/deadline") && !a.contains("heartbeat"));
    let c = with(vec![Trigger::UnlessConfirmed(Duration::new(600))]);
    assert!(c.contains("$INST/deadline") && !c.contains("heartbeat"));
    let h = with(vec![Trigger::UnlessHeartbeat {
        deadline: Duration::new(60),
        interval: Some(Duration::new(20)),
    }]);
    assert!(!h.contains("$INST/deadline") && h.contains("$((now - h)) -gt 60"));
    // No heartbeat file at all is a lost engine, not a fresh one.
    assert!(h.contains("-gt 60 ] && due=1; else due=1; fi"));
    let both = with(vec![
        Trigger::After(Duration::new(3600)),
        Trigger::UnlessHeartbeat {
            deadline: Duration::new(60),
            interval: None,
        },
    ]);
    assert!(both.contains("$INST/deadline") && both.contains("-gt 60"));
}

#[test]
fn every_undo_form_and_primitive_renders() {
    let region = Op {
        undo_locus: UndoLocus::Target,
        ..Op::new(
            "r",
            vec![FootprintEntry::anchored("file:/etc/pf.conf", "blk")],
        )
    };
    let modified = Op {
        undo_locus: UndoLocus::Target,
        drift: Some(Drift::Defer),
        ..Op::new(
            "m",
            vec![FootprintEntry::entry(Kind::Modified, "file:/etc/m")],
        )
    };
    let body = Op {
        undo_locus: UndoLocus::Target,
        undo: Undo::Computed {
            body: vec![
                run(vec![text("svc stop "), interp(param("name"))]),
                write(fact_ref("file:/etc/w"), lit("it's")),
                remove(fact_ref("file:/etc/w")),
                append(fact_ref("file:/etc/log"), Value::Ref(host_field("name"))),
                region_set(anchored_ref("file:/etc/rs", "blk"), lit("c")),
                region_clear(anchored_ref("file:/etc/rs", "blk")),
            ],
            undo_pre: vec!["file:/etc/b".into()],
        },
        ..Op::new(
            "b",
            vec![FootprintEntry::entry(Kind::Owned, "winfw:rule:x")],
        )
    };
    let p = plan("fw", vec![s(region), s(modified), s(body)], after());
    let mut b = Bindings::default();
    b.params.insert("name".into(), "sshd it's".into());
    let a = render(&site(), &p, "fw", &inst(), &b).unwrap().text;
    assert!(a.contains("strip_region '/etc/pf.conf' 'blk' || { if [ -n \"${RUE_NOLOCK:-}\" ] || foreign_region '/etc/pf.conf'; then defer 1; else restore '/var/db/rue/instances/i-1/snapshots/1/0' '/etc/pf.conf'; clobbered 1; fi; }"), "{a}");
    assert!(
        a.contains("[ \"$(sha '/etc/m')\" = \"$(recorded \"$M\" '/etc/m')\" ] || skip=1"),
        "{a}"
    );
    assert!(
        a.contains("restore '/var/db/rue/instances/i-1/snapshots/2/0' '/etc/m'"),
        "{a}"
    );
    assert!(
        a.contains("sh -c 'svc stop '\\''sshd it'\\''\\'\\'''\\''s'\\'''"),
        "{a}"
    );
    assert!(a.contains("printf '%s' 'it'\\''s' > '/etc/w'"), "{a}");
    assert!(a.contains("rm -f '/etc/w'"), "{a}");
    assert!(a.contains("printf '%s\\n' 'fw' >> '/etc/log'"), "{a}");
    assert!(a.contains("region_set '/etc/rs' 'blk' 'c'"), "{a}");
    assert!(a.contains("strip_region '/etc/rs' 'blk' || true"), "{a}");
    // The non-file Owned fact of step 3 gets no drift check.
    assert!(!a.contains("winfw"), "{a}");
    for h in ["win", "py"] {
        let p = plan(h, p.body.to_vec(), after());
        render(&site(), &p, h, &inst(), &b).unwrap_or_else(|e| panic!("{h}: {e}"));
    }
}

#[test]
fn refusals() {
    let no_backstop = Plan {
        backstop: None,
        ..plan("fw", vec![s(owned("a"))], after())
    };
    assert_eq!(render_on("fw", &no_backstop), Err(RenderError::NoBackstop));
    let not_target = plan(
        "fw",
        vec![s(Op {
            undo_locus: UndoLocus::Controller,
            ..owned("a")
        })],
        after(),
    );
    assert_eq!(render_on("fw", &not_target), Err(RenderError::NotTarget));
    let p = plan("fw", vec![s(owned("a"))], after());
    assert_eq!(
        render_on("nope", &p),
        Err(RenderError::UnknownHost("nope".into()))
    );
    let e = render_on("bad", &plan("bad", vec![s(owned("a"))], after())).unwrap_err();
    assert_eq!(e.code(), Some(Code::E0403));
    assert_eq!(
        e.to_string(),
        format!("{}: no sh template for os windows", Code::E0403)
    );

    let with_undo = |undo: Undo, footprint: Vec<FootprintEntry>| {
        plan(
            "fw",
            vec![s(Op {
                undo_locus: UndoLocus::Target,
                undo,
                ..Op::new("x", footprint)
            })],
            after(),
        )
    };
    let owned_file = || vec![FootprintEntry::entry(Kind::Owned, "file:/etc/x")];
    // E0109 from a value in a run string.
    let mut b = Bindings::default();
    b.params.insert("v".into(), "a\u{1}b".into());
    let e = render(
        &site(),
        &with_undo(
            Undo::Computed {
                body: vec![run(vec![text("x "), interp(param("v"))])],
                undo_pre: vec!["file:/etc/x".into()],
            },
            owned_file(),
        ),
        "fw",
        &inst(),
        &b,
    )
    .unwrap_err();
    assert_eq!(e.code(), Some(Code::E0109));
    // Not bakeable: an unbound parameter, a controller value, a secret, an
    // earlier output, a fact read, a controller primitive, env:/stdin:, a
    // held resource under restore.
    let computed = |body: Vec<Prim>| Undo::Computed {
        body,
        undo_pre: vec!["file:/etc/x".into()],
    };
    let unbound = render_on(
        "fw",
        &with_undo(computed(vec![run(vec![interp(param("v"))])]), owned_file()),
    )
    .unwrap_err();
    assert!(
        matches!(unbound, RenderError::Unbound { step: 1, ref name } if name == "v"),
        "{unbound}"
    );
    for r in [
        controller("c"),
        secret("s"),
        output("o", "x", true),
        fact("f"),
    ] {
        let e = render_on(
            "fw",
            &with_undo(computed(vec![run(vec![interp(r.clone())])]), owned_file()),
        )
        .unwrap_err();
        assert!(
            matches!(e, RenderError::NotBakeable { step: 1, .. }),
            "{r:?}: {e}"
        );
    }
    for prim in [
        hook("h", vec![]),
        install("i"),
        release("r"),
        stage("s", lit("x"), 0o600),
    ] {
        let e =
            render_on("fw", &with_undo(computed(vec![prim.clone()]), owned_file())).unwrap_err();
        assert!(
            matches!(e, RenderError::NotBakeable { step: 1, .. }),
            "{prim:?}: {e}"
        );
    }
    let with_stdin = Prim::Run(Run {
        cmd: vec![text("x")],
        env: vec![],
        stdin: Some(lit("y")),
    });
    assert!(matches!(
        render_on("fw", &with_undo(computed(vec![with_stdin]), owned_file())),
        Err(RenderError::NotBakeable { .. })
    ));
    let held = with_undo(
        Undo::Restore,
        vec![FootprintEntry::entry(Kind::Held, "proc:t")],
    );
    let held = Plan {
        body: vec![match held.body.into_iter().next().unwrap() {
            Item::Step(st) => Item::Step(StepI {
                op: Op {
                    suspend: Some(vec![]),
                    reestablish: Some(vec![]),
                    ..st.op
                },
                ..st
            }),
            other => other,
        }],
        ..held
    };
    assert!(matches!(
        render_on("fw", &held),
        Err(RenderError::NotBakeable { .. })
    ));
    // A restore over a non-file or runtime-bound fact cannot be observed.
    for shape in ["winfw:rule:x", "file:/etc/{n}"] {
        let e = render_on(
            "fw",
            &with_undo(
                Undo::Restore,
                vec![FootprintEntry::entry(Kind::Owned, shape)],
            ),
        )
        .unwrap_err();
        assert!(
            matches!(e, RenderError::Unobservable { step: 1, .. }),
            "{shape}: {e}"
        );
    }
    // A run body over a non-file fact is fine: the fact is undone as intact.
    assert!(render_on(
        "fw",
        &with_undo(
            computed(vec![run_lit("rule-delete x")]),
            vec![FootprintEntry::entry(Kind::Owned, "winfw:rule:x")]
        )
    )
    .is_ok());
}

#[test]
fn a_secret_elsewhere_in_the_plan_never_reaches_the_artifact() {
    // Step 2 is uncovered (controller undo) and carries a secret in its do
    // body; step 1 is covered. The artifact holds step 1 only.
    let p = plan(
        "fw",
        vec![
            s(owned("a")),
            s(Op {
                undo_locus: UndoLocus::Controller,
                do_: vec![run(vec![text("login "), interp(secret("db_pw"))])],
                ..owned("b")
            }),
        ],
        after(),
    );
    let a = render_on("fw", &p).unwrap().text;
    assert!(!a.contains("db_pw") && !a.contains("login"));
}
