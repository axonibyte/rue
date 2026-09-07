//! Tier 1 for the checker (a port of the prototype's Test.Check): every code
//! the checker can raise has a plan that raises it and a sibling that does
//! not, and the codes it can raise are stated as a list so the not-proven
//! table is a fact, not a guess.

mod common;

use common::*;
use rue_core::body::*;
use rue_core::check::check;
use rue_core::diagnostics::Code;
use rue_core::model::*;
use rue_core::verdict::*;

fn site0() -> Site {
    Site {
        hosts: vec![
            HostRecord {
                name: "db-01".into(),
                os: "freebsd".into(),
                reach: vec!["ssh".into()],
                filesystem: true,
                stdin_preamble: true,
                artifact: None,
            },
            HostRecord {
                name: "api-01".into(),
                os: "appliance".into(),
                reach: vec!["api".into()],
                filesystem: false,
                stdin_preamble: false,
                artifact: None,
            },
            HostRecord {
                name: "island".into(),
                os: "freebsd".into(),
                reach: vec!["console".into()],
                filesystem: true,
                stdin_preamble: true,
                artifact: None,
            },
        ],
        transports: vec!["ssh".into()],
        authenticators: vec![
            Authenticator {
                id: "oncall".into(),
                human: true,
            },
            Authenticator {
                id: "alice".into(),
                human: true,
            },
            Authenticator {
                id: "driver".into(),
                human: false,
            },
            Authenticator {
                id: "requester".into(),
                human: true,
            },
        ],
        max_wait: None,
        scheduler_present: vec!["db-01".into()],
        secrets_deliver_to: vec![],
    }
}

fn codes_with(site: &Site, p: &Plan) -> Vec<Code> {
    let mut v: Vec<Code> = check(site, "requester", p)
        .diagnostics
        .iter()
        .map(|d| d.code)
        .collect();
    v.sort();
    v
}

fn codes_of(p: &Plan) -> Vec<Code> {
    codes_with(&site0(), p)
}

fn raises(c: Code, p: &Plan) -> bool {
    codes_of(p).contains(&c)
}

fn clean(p: &Plan) -> bool {
    codes_of(p).is_empty()
}

fn pair(code: Code, bad: &Plan, good: &Plan) {
    assert!(raises(code, bad), "did not raise {code}");
    assert!(
        !raises(code, good),
        "raised {code} on the good plan: {:?}",
        codes_of(good)
    );
}

fn target(o: Op) -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        ..o
    }
}

fn reach_op() -> Op {
    Op {
        reach: vec!["ssh".into()],
        ..target(owned("pf"))
    }
}

fn backstop_after_1h() -> Backstop {
    Backstop {
        triggers: vec![Trigger::After(Duration::new(3600))],
        arm_before: 1,
    }
}

fn auth(id: &str) -> GateExpr {
    GateExpr::Single(Factor::Auth {
        id: id.into(),
        weight: 1,
    })
}

fn gated(g: GateExpr) -> Plan {
    Plan {
        gate: Some(PlanGate {
            expr: g,
            window: Some(Duration::new(3600)),
            allow_zero_human: false,
        }),
        ..temp(vec![s(owned("a"))])
    }
}

fn with_gate(o: Op, g: GateExpr) -> Item {
    Item::Step(StepI {
        gate: Some(g),
        ..StepI::new(o)
    })
}

#[test]
fn op_rules() {
    pair(
        Code::E0201,
        &temp(vec![s(Op {
            undo: Undo::NoUndo,
            ..owned("a")
        })]),
        &temp(vec![Item::Knell(StepI::new(knell_op()))]),
    );
    pair(
        Code::E0202,
        &temp(vec![s(Op {
            undo: computed(
                vec![run(vec![text("restore "), interp(controller("backup"))])],
                &["file:/a"],
            ),
            ..target(owned("a"))
        })]),
        &temp(vec![s(target(owned("a")))]),
    );
    pair(
        Code::E0203,
        &temp(vec![s(Op {
            undo_locus: UndoLocus::NoLocus,
            ..owned("a")
        })]),
        &temp(vec![s(owned("a"))]),
    );
    let tunnel = Op::new(
        "tunnel",
        vec![FootprintEntry::entry(Kind::Held, "proc:tunnel")],
    );
    pair(
        Code::E0205,
        &temp(vec![s(tunnel.clone())]),
        &temp(vec![s(Op {
            suspend: Some(vec![run_lit("tunnel suspend")]),
            reestablish: Some(vec![run_lit("tunnel resume")]),
            ..tunnel.clone()
        })]),
    );
    // Suspend without reestablish is half a pair and still E0205.
    assert!(raises(
        Code::E0205,
        &temp(vec![s(Op {
            suspend: Some(vec![run_lit("tunnel suspend")]),
            ..tunnel
        })])
    ));
    pair(
        Code::E0207,
        &temp(vec![s(Op {
            undo: computed(vec![run_lit("restore")], &[]),
            ..owned("a")
        })]),
        &temp(vec![s(Op {
            undo: computed(vec![run_lit("restore")], &["file:/a"]),
            ..owned("a")
        })]),
    );
    pair(
        Code::E0208,
        &temp(vec![s(Op {
            undo_idempotent: false,
            ..owned("a")
        })]),
        &temp(vec![s(owned("a"))]),
    );
    let on_api = |o: Op| Op {
        locus: Locus::Host(HostRef::Static("api-01".into())),
        ..o
    };
    pair(
        Code::E0407,
        &temp(vec![s(on_api(target(owned("a"))))]),
        &temp(vec![s(on_api(owned("a")))]),
    );
    pair(
        Code::E0410,
        &Plan {
            backstop: Some(backstop_after_1h()),
            ..temp(vec![s(Op {
                reach: vec!["ssh".into()],
                ..target(modified("pf"))
            })])
        },
        &Plan {
            backstop: Some(backstop_after_1h()),
            ..temp(vec![s(reach_op())])
        },
    );
}

#[test]
fn interference_rules() {
    pair(
        Code::E0301,
        &temp(vec![s(owned("a")), s(owned("a"))]),
        &temp(vec![s(owned("a")), s(owned("b"))]),
    );
    let any = || {
        s(Op::new(
            "any",
            vec![FootprintEntry::entry(Kind::Owned, "file:/etc/{name}")],
        ))
    };
    let strict = temp(vec![s(owned("etc/x")), any()]);
    pair(
        Code::E0302,
        &strict,
        &Plan {
            strictness: Strictness::Warn,
            ..strict.clone()
        },
    );
    let v = check(
        &site0(),
        "requester",
        &Plan {
            strictness: Strictness::Warn,
            ..strict
        },
    );
    assert_eq!(
        v.may_conflicts.len(),
        1,
        "a may-conflict under :warn is a verdict clause"
    );
    assert_eq!(v.status, Status::Ok);
    pair(
        Code::E0303,
        &temp(vec![Item::Par {
            children: vec![s(owned("a")), s(owned("a"))],
        }]),
        &temp(vec![Item::Par {
            children: vec![s(owned("a")), s(owned("b"))],
        }]),
    );
    assert!(
        !raises(
            Code::E0301,
            &temp(vec![Item::Par {
                children: vec![s(owned("a")), s(owned("a"))]
            }])
        ),
        "par siblings are judged by E0303, never E0301"
    );
    assert!(raises(
        Code::E0301,
        &temp(vec![
            Item::Par {
                children: vec![s(owned("a")), s(owned("b"))]
            },
            s(owned("a"))
        ])
    ));
    pair(
        Code::E0304,
        &Plan {
            backstop: Some(backstop_after_1h()),
            ..temp(vec![Item::Par {
                children: vec![s(reach_op()), s(owned("b"))],
            }])
        },
        &temp(vec![Item::Par {
            children: vec![s(owned("a")), s(owned("b"))],
        }]),
    );
    let r = |id: &str, anchor: &str| {
        s(Op::new(
            id,
            vec![FootprintEntry::anchored("file:/etc/keys", anchor)],
        ))
    };
    pair(
        Code::E0305,
        &temp(vec![r("r1", "rue"), r("r2", "rue")]),
        &temp(vec![r("r1", "rue-a"), r("r2", "rue-b")]),
    );
    assert_eq!(
        codes_of(&temp(vec![r("r1", "rue"), r("r2", "rue")])),
        vec![Code::E0305],
        "a repeated anchor is E0305 alone"
    );
    assert!(!raises(
        Code::E0301,
        &temp(vec![r("r1", "a"), r("r2", "b")])
    ));
    let lp = Item::Repeat {
        form: RepeatForm::Over {
            list: "guests".into(),
            max: 8,
            set_valued: true,
        },
        var: "g".into(),
        body: vec![s(Op::new(
            "stop",
            vec![FootprintEntry::entry(Kind::Modified, "guest:{g}:state")],
        ))],
    };
    assert!(clean(&temp(vec![lp])));
    let other_host = |o: Op| Op {
        locus: Locus::Host(HostRef::Static("api-01".into())),
        undo_locus: UndoLocus::Controller,
        ..o
    };
    assert!(
        !raises(
            Code::E0301,
            &temp(vec![s(owned("a")), s(other_host(owned("a")))])
        ),
        "the same shape on two static hosts is two facts"
    );
    let bound = |o: Op| Op {
        locus: Locus::Host(HostRef::Bound("pick".into())),
        undo_locus: UndoLocus::Controller,
        ..o
    };
    assert!(
        raises(
            Code::E0302,
            &temp(vec![s(owned("a")), s(bound(owned("a")))])
        ),
        "a bound host may conflict"
    );
    assert!(!raises(
        Code::E0303,
        &temp(vec![Item::Par {
            children: vec![s(owned("a")), s(other_host(owned("a")))]
        }])
    ));
}

#[test]
fn backstop_rules() {
    pair(
        Code::E0401,
        &temp(vec![s(reach_op())]),
        &Plan {
            backstop: Some(backstop_after_1h()),
            ..temp(vec![s(reach_op())])
        },
    );
    assert!(raises(
        Code::E0401,
        &Plan {
            backstop: Some(Backstop {
                arm_before: 3,
                ..backstop_after_1h()
            }),
            ..temp(vec![s(target(owned("a"))), s(reach_op())])
        }
    ));
    let covered = || temp(vec![s(target(owned("a")))]);
    pair(
        Code::E0403,
        &Plan {
            backstop: Some(backstop_after_1h()),
            owner: "island".into(),
            ..covered()
        },
        &Plan {
            backstop: Some(backstop_after_1h()),
            ..covered()
        },
    );
    let hb = |i: u64| Backstop {
        triggers: vec![
            Trigger::After(Duration::new(3600)),
            Trigger::UnlessHeartbeat {
                deadline: Duration::new(60),
                interval: Some(Duration::new(i)),
            },
        ],
        arm_before: 1,
    };
    pair(
        Code::E0405,
        &Plan {
            backstop: Some(hb(30)),
            ..covered()
        },
        &Plan {
            backstop: Some(hb(20)),
            ..covered()
        },
    );
    pair(
        Code::E0503,
        &Plan {
            backstop: Some(Backstop {
                triggers: vec![Trigger::After(Duration::new(7200))],
                arm_before: 1,
            }),
            ..covered()
        },
        &Plan {
            backstop: Some(backstop_after_1h()),
            ..covered()
        },
    );
    assert!(raises(
        Code::E0503,
        &Plan {
            backstop: Some(Backstop {
                triggers: vec![
                    Trigger::After(Duration::new(3600)),
                    Trigger::UnlessConfirmed(Duration::new(600))
                ],
                arm_before: 1
            }),
            ..covered()
        }
    ));
    let confirmed = Backstop {
        triggers: vec![Trigger::UnlessConfirmed(Duration::new(600))],
        arm_before: 1,
    };
    pair(
        Code::E0504,
        &Plan {
            backstop: Some(backstop_after_1h()),
            ..Plan::new("p", "db-01", vec![s(target(owned("a"))), Item::Commit])
        },
        &Plan {
            backstop: Some(confirmed),
            ..Plan::new(
                "p",
                "db-01",
                vec![s(target(owned("a"))), Item::Confirm, Item::Commit],
            )
        },
    );
}

#[test]
fn mode_rules() {
    let forced = temp(vec![Item::Step(StepI {
        force: vec![ForceName::Unknown],
        ..StepI::new(owned("a"))
    })]);
    pair(
        Code::E0404,
        &Plan {
            mode: Mode::Auto,
            ..forced.clone()
        },
        &forced,
    );
    let human_ack = Op {
        refusal: Refusal::Knell {
            guard: None,
            cost: Cost::NoCost("n/a".into()),
            ack: Ack::Gate(GateExpr::Single(Factor::Humans { weight: 1 })),
        },
        ..knell_op()
    };
    pair(
        Code::E0507,
        &Plan {
            mode: Mode::Auto,
            ..temp(vec![Item::Knell(StepI::new(human_ack))])
        },
        &Plan {
            mode: Mode::Auto,
            ..temp(vec![Item::Knell(StepI::new(knell_op()))])
        },
    );
    assert!(
        raises(
            Code::E0507,
            &Plan {
                mode: Mode::Auto,
                ..temp(vec![with_gate(owned("a"), auth("oncall"))])
            }
        ),
        "a human step gate under :auto"
    );
}

#[test]
fn intent_rules() {
    pair(
        Code::E0501,
        &temp(vec![s(owned("a")), Item::Commit]),
        &temp(vec![s(owned("a"))]),
    );
    assert!(
        raises(Code::E0501, &Plan::new("p", "db-01", vec![s(owned("a"))])),
        "neither wane nor commit"
    );
    pair(
        Code::E0502,
        &Plan::new("p", "db-01", vec![Item::Commit, s(owned("a"))]),
        &Plan::new("p", "db-01", vec![s(owned("a")), Item::Commit]),
    );
    let branch = |then_: Vec<Item>, else_: Vec<Item>| Item::When {
        guard: Guard::new("g", Tri::Yes),
        window: None,
        on_lapse: OnLapse::Revert,
        then_,
        else_,
    };
    pair(
        Code::E0505,
        &Plan::new("p", "db-01", vec![branch(vec![Item::Commit], vec![])]),
        &Plan::new(
            "p",
            "db-01",
            vec![branch(vec![Item::Commit], vec![Item::Commit])],
        ),
    );
    let fbc = Plan {
        fires_by_construction: true,
        backstop: Some(Backstop {
            triggers: vec![Trigger::UnlessConfirmed(Duration::new(600))],
            arm_before: 1,
        }),
        ..Plan::new("p", "db-01", vec![s(target(owned("a")))])
    };
    let v = check(&site0(), "requester", &fbc);
    assert_eq!(
        v.wane,
        Some(Duration::new(600)),
        "fires_by_construction is temporary with the confirmed duration as wane"
    );
    assert_eq!(v.status, Status::Ok);
}

#[test]
fn wait_rules() {
    pair(
        Code::E0506,
        &Plan::new(
            "p",
            "db-01",
            vec![with_gate(owned("a"), auth("oncall")), Item::Commit],
        ),
        &Plan::new(
            "p",
            "db-01",
            vec![
                Item::Step(StepI {
                    gate: Some(auth("oncall")),
                    window: Some(Duration::new(900)),
                    ..StepI::new(owned("a"))
                }),
                Item::Commit,
            ],
        ),
    );
    assert!(
        raises(
            Code::E0506,
            &Plan {
                gate: Some(PlanGate {
                    expr: auth("oncall"),
                    window: None,
                    allow_zero_human: false
                }),
                ..temp(vec![s(owned("a"))])
            }
        ),
        "a plan-entry gate with no window and no max_wait: Pending unbounded"
    );
    let bounded_site = Site {
        max_wait: Some(Duration::new(1800)),
        ..site0()
    };
    let waits = Plan::new(
        "p",
        "db-01",
        vec![
            Item::Assert {
                guard: Guard::new("g", Tri::Yes),
                window: None,
                on_lapse: OnLapse::Revert,
            },
            Item::Commit,
        ],
    );
    assert!(
        !codes_with(&bounded_site, &waits).contains(&Code::E0506),
        "the site's max_wait bounds a permanent plan's waits"
    );
    let fbc_wait = Plan {
        fires_by_construction: true,
        backstop: Some(Backstop {
            triggers: vec![Trigger::UnlessHeartbeat {
                deadline: Duration::new(60),
                interval: None,
            }],
            arm_before: 1,
        }),
        ..Plan::new(
            "p",
            "db-01",
            vec![
                Item::Assert {
                    guard: Guard::new("g", Tri::Yes),
                    window: None,
                    on_lapse: OnLapse::Revert,
                },
                s(target(owned("a"))),
            ],
        )
    };
    assert!(
        raises(Code::E0506, &fbc_wait),
        "no confirmed trigger, no wane, can wait"
    );
}

#[test]
fn gate_rules() {
    pair(Code::E0508, &gated(auth("nobody")), &gated(auth("oncall")));
    assert!(
        raises(
            Code::E0508,
            &gated(GateExpr::Thresh {
                n: 5,
                factors: vec![Factor::Auth {
                    id: "oncall".into(),
                    weight: 1
                }]
            })
        ),
        "unsatisfiable threshold"
    );
    assert!(
        raises(Code::E0508, &gated(auth("requester"))),
        "a gate counting the requester"
    );
    let self_ack = Op {
        refusal: Refusal::Knell {
            guard: None,
            cost: Cost::NoCost("n/a".into()),
            ack: Ack::Gate(auth("requester")),
        },
        ..knell_op()
    };
    assert!(
        !raises(Code::E0508, &temp(vec![Item::Knell(StepI::new(self_ack))])),
        "the requester may acknowledge a knell"
    );
    let wait_gate = GateExpr::Single(Factor::Wait {
        duration: Duration::new(1800),
        weight: 1,
    });
    pair(
        Code::E0509,
        &gated(wait_gate.clone()),
        &Plan {
            gate: Some(PlanGate {
                expr: wait_gate,
                window: Some(Duration::new(3600)),
                allow_zero_human: true,
            }),
            ..temp(vec![s(owned("a"))])
        },
    );
    let v = check(
        &site0(),
        "requester",
        &gated(GateExpr::Thresh {
            n: 2,
            factors: vec![
                Factor::Auth {
                    id: "oncall".into(),
                    weight: 1,
                },
                Factor::Auth {
                    id: "alice".into(),
                    weight: 1,
                },
                Factor::Auth {
                    id: "driver".into(),
                    weight: 1,
                },
            ],
        }),
    );
    assert_eq!(
        v.gate.as_ref().and_then(|g| g.min_distinct_humans),
        Some(1),
        "minimum distinct humans over satisfying paths"
    );
}

#[test]
fn verdict_shape() {
    let v = check(
        &site0(),
        "requester",
        &temp(vec![s(owned("a")), s(owned("b"))]),
    );
    assert_eq!(v.status, Status::Ok);
    assert_eq!(v.reversible_through, 2);
    assert_eq!(v.controller_only_undos, vec![1, 2]);
    assert_eq!(v.mode, "manual");

    let hold = Op {
        refusal: Refusal::Hold { via: None },
        ..owned("b")
    };
    let v = check(
        &site0(),
        "requester",
        &temp(vec![
            s(owned("a")),
            Item::Knell(StepI::new(knell_op())),
            s(hold),
        ]),
    );
    assert_eq!(v.reversible_through, 1, "a knell is the point of no return");
    assert_eq!(v.point_of_no_return.as_ref().map(|p| p.step), Some(2));
    assert_eq!(v.reversible_back_to, Some((3, 2)));
    assert_eq!(v.holds_at, vec![3]);

    let island = Op {
        locus: Locus::Host(HostRef::Static("island".into())),
        ..owned("a")
    };
    assert_eq!(
        check(&site0(), "requester", &temp(vec![s(island)])).deferred,
        vec![1],
        "a step on an unreachable host is deferred"
    );

    let controller = Op {
        locus: Locus::Controller,
        ..owned("a")
    };
    assert_eq!(
        check(&site0(), "requester", &temp(vec![s(controller)])).hosts_touched,
        vec![(
            1,
            vec![HostTouched::Host {
                host: "controller".into(),
                directory: "controller".into()
            }]
        )],
        "a :controller step touches the controller, not the owner host"
    );

    let bound = Op {
        locus: Locus::Host(HostRef::Bound("pick".into())),
        ..owned("a")
    };
    let v = check(&site0(), "requester", &temp(vec![s(bound)]));
    assert_eq!(v.unresolved_bindings, vec!["pick".to_string()]);
    assert_eq!(
        v.hosts_touched,
        vec![(1, vec![HostTouched::Unresolved("bound from pick".into())])]
    );
}

#[test]
fn a_conflict_is_reported_once_per_pair_and_fact() {
    // Two entries of one shape in one footprint still name one fact.
    let doubled = Op::new(
        "a",
        vec![
            FootprintEntry::entry(Kind::Owned, "file:/a"),
            FootprintEntry::entry(Kind::Owned, "file:/a"),
        ],
    );
    assert_eq!(
        codes_of(&temp(vec![s(doubled), s(owned("a"))])),
        vec![Code::E0301]
    );
}

#[test]
fn diagnostics_are_sorted_by_code_and_stable_within_a_code() {
    // Two E0201s in step order, then an E0501 that was generated later.
    let p = Plan::new(
        "p",
        "db-01",
        vec![
            s(Op {
                undo: Undo::NoUndo,
                ..owned("a")
            }),
            s(Op {
                undo: Undo::NoUndo,
                ..owned("b")
            }),
        ],
    );
    let v = check(&site0(), "requester", &p);
    let seq: Vec<(Code, Option<u32>)> = v.diagnostics.iter().map(|d| (d.code, d.step)).collect();
    assert_eq!(
        seq,
        vec![
            (Code::E0201, Some(1)),
            (Code::E0201, Some(2)),
            (Code::E0501, None)
        ]
    );
}

/// E0202 is computed from the undo body (docs/ROADMAP.md 5.3): a `:target`
/// undo is closed iff a target-side artifact could carry it out alone.
#[test]
fn closure_of_a_target_undo_follows_the_body() {
    let closed = |undo: Undo| -> bool {
        !raises(
            Code::E0202,
            &temp(vec![s(Op {
                undo,
                ..target(owned("a"))
            })]),
        )
    };
    // Target-local references: a fact, a plan parameter, a host field.
    assert!(closed(computed(
        vec![
            run(vec![text("restore "), interp(fact("snapshot"))]),
            write(fact_ref("file:/a"), Value::Ref(param("posture"))),
            run(vec![text("ping "), interp(host_field("address"))]),
        ],
        &["file:/a"],
    )));
    // A controller value in any target-local position.
    assert!(!closed(computed(
        vec![write(fact_ref("file:/a"), Value::Ref(controller("c")))],
        &["file:/a"],
    )));
    assert!(!closed(computed(
        vec![Prim::Run(Run {
            cmd: vec![text("restore")],
            env: vec![EnvVar {
                name: "FROM".into(),
                value: Value::Ref(controller("c")),
            }],
            stdin: None,
        })],
        &["file:/a"],
    )));
    assert!(!closed(computed(
        vec![Prim::Run(Run {
            cmd: vec![text("restore")],
            env: vec![],
            stdin: Some(Value::Ref(output("bmc", "token", false))),
        })],
        &["file:/a"],
    )));
    // An earlier step's output is never persisted to the target.
    assert!(!closed(computed(
        vec![run(vec![
            text("use "),
            interp(output("bmc", "port", false))
        ])],
        &["file:/a"],
    )));
    // Controller primitives.
    for prim in [hook("h", vec![]), install("x"), release("x")] {
        assert!(!closed(computed(vec![prim], &["file:/a"])));
    }
    // A call follows its declared classes.
    let call = |class: ArgClass| {
        Prim::Call(Call {
            prim: "svc".into(),
            run: vec![text("service restart")],
            args: vec![ClassedArg {
                name: "n".into(),
                class,
                value: lit("sshd"),
            }],
        })
    };
    assert!(closed(computed(
        vec![call(ArgClass::TargetLocal)],
        &["file:/a"]
    )));
    assert!(!closed(computed(
        vec![call(ArgClass::Controller)],
        &["file:/a"]
    )));
    // A runtime-bound fact shape.
    assert!(!closed(computed(
        vec![remove(fact_ref("file:/{n}"))],
        &["file:/a"],
    )));
    // A compensating body is judged the same way.
    assert!(!closed(compensate(vec![hook("h", vec![])], &["file:/a"])));
    // A secret is E0210's finding, not E0202's.
    assert!(closed(computed(
        vec![run(vec![text("login "), interp(secret("pw"))])],
        &["file:/a"],
    )));
    // Restore: closed over static shapes; not over a runtime shape or a hold.
    assert!(!raises(
        Code::E0202,
        &temp(vec![s(target(Op::new(
            "r",
            vec![
                FootprintEntry::entry(Kind::Owned, "file:/a"),
                FootprintEntry::anchored("file:/b", "x"),
                FootprintEntry::entry(Kind::Modified, "file:/c"),
            ],
        )))]),
    ));
    assert!(raises(
        Code::E0202,
        &temp(vec![s(target(Op::new(
            "r",
            vec![FootprintEntry::entry(Kind::Owned, "file:/{n}")],
        )))]),
    ));
    assert!(raises(
        Code::E0202,
        &temp(vec![s(Op {
            suspend: Some(vec![]),
            reestablish: Some(vec![]),
            ..target(Op::new(
                "h",
                vec![FootprintEntry::entry(Kind::Held, "proc:t")],
            ))
        })]),
    ));
    // The same bodies are closed enough for a :controller undo locus.
    assert!(!raises(
        Code::E0202,
        &temp(vec![s(Op {
            undo: computed(vec![hook("h", vec![])], &["file:/a"]),
            ..owned("a")
        })]),
    ));
    // The message names what was found.
    let v = check(
        &site0(),
        "requester",
        &temp(vec![s(Op {
            undo: computed(
                vec![run_lit("ok"), hook("bmc_disable", vec![])],
                &["file:/a"],
            ),
            ..target(owned("a"))
        })]),
    );
    let m = &v
        .diagnostics
        .iter()
        .find(|d| d.code == Code::E0202)
        .unwrap()
        .message;
    assert!(
        m.ends_with("hook is a controller primitive (prim 2)"),
        "{m}"
    );
}

/// Secret placement (docs/ROADMAP.md 5.8) as far as the model decides it.
#[test]
fn secret_rules() {
    let pw = || Value::Ref(secret("pw"));
    let run_with = |cmd: &str, env: Option<Value>, stdin: Option<Value>| {
        Prim::Run(Run {
            cmd: vec![text(cmd)],
            env: env
                .into_iter()
                .map(|value| EnvVar {
                    name: "PW".into(),
                    value,
                })
                .collect(),
            stdin,
        })
    };
    let string_secret = || run(vec![text("login "), interp(secret("pw"))]);
    let delivering = Site {
        secrets_deliver_to: vec!["requester".into()],
        ..site0()
    };

    // E0209: a secret in the string of a run, in any body of the op.
    pair(
        Code::E0209,
        &temp(vec![s(Op {
            do_: vec![string_secret()],
            ..owned("a")
        })]),
        &temp(vec![s(Op {
            do_: vec![run_with("login", Some(pw()), Some(pw()))],
            ..owned("a")
        })]),
    );
    for body in ["undo", "suspend", "reestablish"] {
        let mut o = Op::new("t", vec![FootprintEntry::entry(Kind::Held, "proc:t")]);
        o.suspend = Some(vec![]);
        o.reestablish = Some(vec![]);
        match body {
            "undo" => o.undo = computed(vec![string_secret()], &["proc:t"]),
            "suspend" => o.suspend = Some(vec![string_secret()]),
            _ => o.reestablish = Some(vec![string_secret()]),
        }
        assert!(raises(Code::E0209, &temp(vec![s(o)])), "{body}");
    }
    // An earlier step's secret output counts; a plain output does not.
    assert!(raises(
        Code::E0209,
        &temp(vec![s(Op {
            do_: vec![run(vec![interp(output("bmc", "pw", true))])],
            ..owned("a")
        })])
    ));
    assert!(!raises(
        Code::E0209,
        &temp(vec![s(Op {
            do_: vec![run(vec![interp(output("bmc", "port", false))])],
            ..owned("a")
        })])
    ));

    // E0210: a secret anywhere in a :target undo, and not also E0202.
    let target_undo_with = |stdin: Option<Value>| {
        temp(vec![s(Op {
            undo: computed(vec![run_with("restore", None, stdin)], &["file:/a"]),
            ..target(owned("a"))
        })])
    };
    pair(
        Code::E0210,
        &target_undo_with(Some(pw())),
        &target_undo_with(None),
    );
    assert_eq!(codes_of(&target_undo_with(Some(pw()))), vec![Code::E0210]);
    assert!(!raises(
        Code::E0210,
        &temp(vec![s(Op {
            undo: computed(vec![run_with("restore", None, Some(pw()))], &["file:/a"]),
            ..owned("a")
        })])
    ));

    // E0211: env:/stdin: on a static host whose record cannot carry it.
    let on = |locus: Locus, prim: Prim| {
        temp(vec![s(Op {
            locus,
            do_: vec![prim],
            ..Op::new("m", vec![FootprintEntry::entry(Kind::Modified, "x:y")])
        })])
    };
    let api = || Locus::Host(HostRef::Static("api-01".into()));
    pair(
        Code::E0211,
        &on(api(), run_with("login", Some(pw()), None)),
        &on(api(), run_with("login", None, None)),
    );
    assert!(raises(
        Code::E0211,
        &on(api(), run_with("login", None, Some(pw())))
    ));
    assert!(!raises(
        Code::E0211,
        &on(Locus::Target, run_with("login", Some(pw()), None))
    ));
    assert!(!raises(
        Code::E0211,
        &on(
            Locus::Host(HostRef::Static("db-01".into())),
            run_with("login", Some(pw()), None)
        )
    ));
    assert!(!raises(
        Code::E0211,
        &on(Locus::Controller, run_with("login", Some(pw()), None))
    ));
    assert!(!raises(
        Code::E0211,
        &on(
            Locus::Host(HostRef::Bound("pick".into())),
            run_with("login", Some(pw()), None)
        )
    ));
    // The record's preamble flag decides, not its filesystem: a hook executor
    // may have a filesystem and no shim, an appliance the reverse.
    let mut odd = site0();
    odd.hosts.push(HostRecord {
        name: "fs-no-shim".into(),
        os: "freebsd".into(),
        reach: vec!["ssh".into()],
        filesystem: true,
        stdin_preamble: false,
        artifact: None,
    });
    odd.hosts.push(HostRecord {
        name: "shim-no-fs".into(),
        os: "appliance".into(),
        reach: vec!["api".into()],
        filesystem: false,
        stdin_preamble: true,
        artifact: None,
    });
    let on_host = |h: &str| {
        on(
            Locus::Host(HostRef::Static(h.into())),
            run_with("login", Some(pw()), None),
        )
    };
    assert!(codes_with(&odd, &on_host("fs-no-shim")).contains(&Code::E0211));
    assert!(!codes_with(&odd, &on_host("shim-no-fs")).contains(&Code::E0211));
    let v = check(
        &site0(),
        "requester",
        &on(api(), run_with("login", Some(pw()), None)),
    );
    let m = &v
        .diagnostics
        .iter()
        .find(|d| d.code == Code::E0211)
        .unwrap()
        .message;
    assert_eq!(
        m,
        "op m: secret pw via env: on api-01, whose executor cannot honor the stdin preamble (do, prim 1)"
    );

    // E0206: a reestablish that re-runs a do primitive of a secret-producing op.
    let tunnel = |reestablish: Vec<Prim>, secret_out: bool| {
        temp(vec![s(Op {
            do_: vec![run_lit("tunnel up"), run_lit("tunnel announce")],
            undo: computed(vec![run_lit("tunnel down")], &["proc:t"]),
            suspend: Some(vec![run_lit("tunnel suspend")]),
            reestablish: Some(reestablish),
            outputs: vec![Output {
                name: "tok".into(),
                secret: secret_out,
            }],
            ..Op::new("t", vec![FootprintEntry::entry(Kind::Held, "proc:t")])
        })])
    };
    assert!(
        !codes_with(&delivering, &tunnel(vec![run_lit("tunnel resume")], true))
            .contains(&Code::E0206)
    );
    assert!(codes_with(
        &delivering,
        &tunnel(
            vec![run_lit("tunnel resume"), run_lit("tunnel announce")],
            true
        )
    )
    .contains(&Code::E0206));
    assert!(
        !codes_with(&delivering, &tunnel(vec![run_lit("tunnel up")], false)).contains(&Code::E0206)
    );

    // E0606: a secret output with no acceptor declared.
    let token = |secret_out: bool| {
        temp(vec![s(Op {
            outputs: vec![Output {
                name: "tok".into(),
                secret: secret_out,
            }],
            ..owned("a")
        })])
    };
    assert_eq!(codes_of(&token(true)), vec![Code::E0606]);
    assert!(clean(&token(false)));
    assert!(codes_with(&delivering, &token(true)).is_empty());
}

/// The artifact language of a `:target` backstop's host (sections 4.5 and
/// 7.7): declared or native, and E0403 when the pair has no template.
#[test]
fn artifact_language_rules() {
    use rue_core::artifact::{default_language, language_of, shell_of, supported, Shell};
    assert_eq!(shell_of("windows"), Shell::Powershell);
    for os in ["freebsd", "linux", "macos", "appliance", "reactive-host"] {
        assert_eq!(shell_of(os), Shell::Posix, "{os}");
        assert_eq!(default_language(os), ArtifactLanguage::Sh, "{os}");
    }
    assert_eq!(default_language("windows"), ArtifactLanguage::Powershell);
    assert!(supported("freebsd", ArtifactLanguage::Sh));
    assert!(supported("macos", ArtifactLanguage::Sh));
    assert!(!supported("windows", ArtifactLanguage::Sh));
    assert!(supported("windows", ArtifactLanguage::Powershell));
    assert!(!supported("freebsd", ArtifactLanguage::Powershell));
    assert!(!supported("macos", ArtifactLanguage::Powershell));
    for os in ["freebsd", "linux", "macos", "windows"] {
        assert!(supported(os, ArtifactLanguage::Python), "{os}");
    }

    let host = |os: &str, artifact: Option<ArtifactLanguage>| HostRecord {
        name: "fw".into(),
        os: os.into(),
        reach: vec!["ssh".into()],
        filesystem: true,
        stdin_preamble: true,
        artifact,
    };
    assert_eq!(
        language_of(&host("windows", None)),
        ArtifactLanguage::Powershell
    );
    assert_eq!(
        language_of(&host("windows", Some(ArtifactLanguage::Python))),
        ArtifactLanguage::Python
    );

    // A plan on `fw` with a :target backstop covering one step.
    let site_with = |h: HostRecord| Site {
        hosts: vec![h],
        scheduler_present: vec!["fw".into()],
        ..site0()
    };
    let covered = Plan {
        backstop: Some(backstop_after_1h()),
        ..Plan {
            wane: Some(Duration::new(3600)),
            ..Plan::new("p", "fw", vec![s(target(owned("a")))])
        }
    };
    let e0403 = |h: HostRecord| codes_with(&site_with(h), &covered).contains(&Code::E0403);
    assert!(!e0403(host("freebsd", None)));
    assert!(!e0403(host("macos", None)));
    assert!(!e0403(host("windows", None)));
    assert!(!e0403(host("windows", Some(ArtifactLanguage::Python))));
    assert!(!e0403(host("linux", Some(ArtifactLanguage::Python))));
    assert!(e0403(host("windows", Some(ArtifactLanguage::Sh))));
    assert!(e0403(host("freebsd", Some(ArtifactLanguage::Powershell))));
    assert!(e0403(host("macos", Some(ArtifactLanguage::Powershell))));
    // Only a :target backstop is an artifact: a controller-undo plan with a
    // backstop covers nothing, and a plan without a backstop has none.
    let uncovered = Plan {
        body: vec![s(owned("a"))],
        ..covered.clone()
    };
    assert!(!e0403_on(
        &site_with(host("windows", Some(ArtifactLanguage::Sh))),
        &uncovered
    ));
    let no_backstop = Plan {
        backstop: None,
        ..covered.clone()
    };
    assert!(!e0403_on(
        &site_with(host("windows", Some(ArtifactLanguage::Sh))),
        &no_backstop
    ));
    let v = check(
        &site_with(host("windows", Some(ArtifactLanguage::Sh))),
        "requester",
        &covered,
    );
    let m = &v
        .diagnostics
        .iter()
        .find(|d| d.code == Code::E0403)
        .unwrap()
        .message;
    assert_eq!(m, "backstop artifact: no sh template for os windows on fw");
}

fn e0403_on(site: &Site, p: &Plan) -> bool {
    codes_with(site, p).contains(&Code::E0403)
}
