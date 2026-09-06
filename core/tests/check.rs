//! Tier 1 for the checker (a port of the prototype's Test.Check): every code
//! the checker can raise has a plan that raises it and a sibling that does
//! not, and the codes it can raise are stated as a list so the not-proven
//! table is a fact, not a guess.

mod common;

use common::*;
use rue_core::check::check;
use rue_core::diagnostics::Code;
use rue_core::model::*;
use rue_core::verdict::*;

/// The codes the checker emits. Every other code in the table is a surface,
/// engine or analysis rule this crate does not model yet.
pub const EMITTED_CODES: &[Code] = &[
    Code::E0201,
    Code::E0202,
    Code::E0203,
    Code::E0205,
    Code::E0207,
    Code::E0208,
    Code::E0301,
    Code::E0302,
    Code::E0303,
    Code::E0304,
    Code::E0305,
    Code::E0401,
    Code::E0403,
    Code::E0404,
    Code::E0405,
    Code::E0407,
    Code::E0410,
    Code::E0501,
    Code::E0502,
    Code::E0503,
    Code::E0504,
    Code::E0505,
    Code::E0506,
    Code::E0507,
    Code::E0508,
    Code::E0509,
];

fn site0() -> Site {
    Site {
        hosts: vec![
            HostRecord {
                name: "db-01".into(),
                os: "freebsd".into(),
                reach: vec!["ssh".into()],
                filesystem: true,
            },
            HostRecord {
                name: "api-01".into(),
                os: "appliance".into(),
                reach: vec!["api".into()],
                filesystem: false,
            },
            HostRecord {
                name: "island".into(),
                os: "freebsd".into(),
                reach: vec!["console".into()],
                filesystem: true,
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
        undo_closed: true,
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
            undo_closed: false,
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
            has_suspend: true,
            ..tunnel
        })]),
    );
    pair(
        Code::E0207,
        &temp(vec![s(Op {
            undo: Undo::Computed(vec![]),
            ..owned("a")
        })]),
        &temp(vec![s(Op {
            undo: Undo::Computed(vec!["file:/a".into()]),
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

#[test]
fn the_emitted_code_list_is_sorted_and_duplicate_free() {
    let mut sorted = EMITTED_CODES.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, EMITTED_CODES);
}
