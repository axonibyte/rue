//! The negative cases: plans the checker must refuse with exactly one named
//! code, each a golden under `tenants/_negative/`. The first twelve are
//! docs/ROADMAP.md Phase 0 task 8, derived from T1 and T3 by one change
//! each; the rest cover every other code the checker emits, in the case
//! table's order.

use rue_core::diagnostics::Code;
use rue_core::model::*;

use super::common::*;
use super::{t1, t2, t3};

fn neg(code: Code, slug: &str, site: Site, plan: Plan) -> Negative {
    Negative {
        code,
        slug: slug.into(),
        site,
        requester: "requester".into(),
        plan,
    }
}

/// A small site for the cases no tenant motivates: a filesystem host with
/// ssh, an API appliance with no filesystem, and a host reachable only by
/// console; no max_wait; a scheduler on db-01 only.
pub fn lab() -> Site {
    Site {
        hosts: vec![
            host("db-01", "freebsd", &["ssh"], true),
            host("api-01", "appliance", &["api"], false),
            host("island", "freebsd", &["console"], true),
        ],
        transports: strings(&["ssh"]),
        authenticators: vec![
            authenticator("oncall", true),
            authenticator("alice", true),
            authenticator("driver", false),
        ],
        max_wait: None,
        scheduler_present: strings(&["db-01"]),
    }
}

/// A one-hour temporary plan on db-01.
fn temp(name: &str, items: Vec<Item>) -> Plan {
    Plan {
        wane: Some(dur(3600)),
        ..Plan::new(name, "db-01", items)
    }
}

fn owned(f: &str) -> Op {
    Op::new(
        f,
        vec![FootprintEntry::entry(
            Kind::Owned,
            &format!("file:/etc/{f}"),
        )],
    )
}

fn target_undo(o: Op) -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        ..o
    }
}

fn after_1h() -> Backstop {
    backstop(vec![Trigger::After(dur(3600))], 1)
}

fn fence_knell(ack: Ack) -> Op {
    Op {
        undo: Undo::NoUndo,
        undo_locus: UndoLocus::NoLocus,
        refusal: Refusal::Knell {
            guard: Some(Guard::new("verified_off", Tri::Yes)),
            cost: Cost::Probe("fence_verdict".into()),
            ack,
        },
        ..Op::new("fence", vec![])
    }
}

pub fn cases() -> Vec<Negative> {
    let t1_site = t1::site();
    let t3_site = t3::site();
    vec![
        // docs/ROADMAP.md Phase 0 task 8, in its order.
        // T3 with the backstop armed after the change: the classic self-lockout.
        neg(
            Code::E0401,
            "backstop-armed-after-reach",
            t3_site.clone(),
            Plan {
                backstop: Some(backstop(vec![Trigger::UnlessConfirmed(dur(600))], 2)),
                ..t3::open_mgmt_port()
            },
        ),
        // T3 with the undo locus moved to the controller.
        neg(
            Code::E0401,
            "reach-with-controller-undo",
            t3_site.clone(),
            Plan {
                body: t3::body_with(Op {
                    undo_locus: UndoLocus::Controller,
                    ..t3::pf_allow()
                }),
                ..t3::open_mgmt_port()
            },
        ),
        // An auto plan with a force: on a step.
        neg(
            Code::E0404,
            "auto-with-force",
            t1_site.clone(),
            Plan {
                mode: Mode::Auto,
                gate: None,
                body: vec![Item::Step(StepI {
                    force: vec![ForceName::Unknown],
                    ..StepI::new(Op::new(
                        "posture",
                        vec![FootprintEntry::entry(Kind::Owned, "file:/etc/x")],
                    ))
                })],
                ..t1::breakglass()
            },
        ),
        // T3 with a modified footprint under reach, drift defaulting to :defer.
        neg(
            Code::E0410,
            "reach-with-defer",
            t3_site.clone(),
            Plan {
                body: t3::body_with(Op {
                    footprint: vec![FootprintEntry::entry(Kind::Modified, "file:/etc/pf.conf")],
                    ..t3::pf_allow()
                }),
                ..t3::open_mgmt_port()
            },
        ),
        // Both wane and commit().
        neg(
            Code::E0501,
            "wane-and-commit",
            t3_site.clone(),
            Plan {
                wane: Some(dur(3600)),
                ..t3::open_mgmt_port()
            },
        ),
        // Neither wane nor commit().
        neg(
            Code::E0501,
            "neither-wane-nor-commit",
            t1_site.clone(),
            Plan {
                wane: None,
                backstop: None,
                gate: None,
                ..t1::breakglass()
            },
        ),
        // commit() not last on its path.
        neg(
            Code::E0502,
            "commit-not-last",
            t3_site.clone(),
            Plan {
                body: vec![
                    s(t3::pf_allow()),
                    Item::Commit,
                    Item::Observe {
                        probe: "verify_reach".into(),
                        alias: "reach".into(),
                    },
                    Item::Confirm,
                ],
                ..t3::open_mgmt_port()
            },
        ),
        // A permanent plan whose else arm never commits.
        neg(
            Code::E0505,
            "path-without-commit",
            t3_site.clone(),
            Plan {
                body: vec![
                    s(t3::pf_allow()),
                    Item::Observe {
                        probe: "verify_reach".into(),
                        alias: "reach".into(),
                    },
                    Item::Confirm,
                    Item::When {
                        guard: Guard::new("healthy", Tri::Yes),
                        window: None,
                        on_lapse: OnLapse::Revert,
                        then_: vec![Item::Commit],
                        else_: vec![],
                    },
                ],
                ..t3::open_mgmt_port()
            },
        ),
        // A permanent plan whose step gate has no window and whose site has no max_wait.
        neg(
            Code::E0506,
            "unbounded-wait",
            t3_site.clone(),
            Plan {
                body: vec![
                    Item::Step(StepI {
                        gate: Some(auth("netops")),
                        ..StepI::new(t3::pf_allow())
                    }),
                    Item::Observe {
                        probe: "verify_reach".into(),
                        alias: "reach".into(),
                    },
                    Item::Confirm,
                    Item::Commit,
                ],
                ..t3::open_mgmt_port()
            },
        ),
        // An auto plan whose knell wants a human acknowledgement (T2's site,
        // so the wait itself is bounded by max_wait).
        neg(
            Code::E0507,
            "auto-with-human-ack",
            t2::site(),
            Plan {
                mode: Mode::Auto,
                ..Plan::new(
                    "promote",
                    "node-b",
                    vec![knell(fence_knell(Ack::Gate(humans()))), Item::Commit],
                )
            },
        ),
        // A gate that counts the requester.
        neg(
            Code::E0508,
            "gate-counts-requester",
            Site {
                authenticators: [
                    t1_site.authenticators.clone(),
                    vec![authenticator("requester", true)],
                ]
                .concat(),
                ..t1_site.clone()
            },
            Plan {
                gate: Some(plan_gate(auth("requester"), Some(1800), false)),
                ..t1::breakglass()
            },
        ),
        // A gate satisfiable by waiting alone.
        neg(
            Code::E0509,
            "zero-human-gate",
            t1_site.clone(),
            Plan {
                gate: Some(plan_gate(wait(1800), Some(3600), false)),
                ..t1::breakglass()
            },
        ),
        // Op rules (section 5.3).
        neg(
            Code::E0201,
            "no-undo-not-knell",
            lab(),
            temp(
                "posture",
                vec![s(Op {
                    undo: Undo::NoUndo,
                    ..owned("a")
                })],
            ),
        ),
        neg(
            Code::E0202,
            "target-undo-not-closed",
            lab(),
            temp(
                "posture",
                vec![s(Op {
                    undo_closed: false,
                    ..target_undo(owned("a"))
                })],
            ),
        ),
        neg(
            Code::E0203,
            "none-locus-with-undo",
            lab(),
            temp(
                "posture",
                vec![s(Op {
                    undo_locus: UndoLocus::NoLocus,
                    ..owned("a")
                })],
            ),
        ),
        neg(
            Code::E0205,
            "held-without-suspend",
            lab(),
            temp(
                "tunnel",
                vec![s(Op::new(
                    "tunnel",
                    vec![FootprintEntry::entry(Kind::Held, "proc:tunnel")],
                ))],
            ),
        ),
        neg(
            Code::E0207,
            "compensate-without-undo-pre",
            lab(),
            temp(
                "posture",
                vec![s(Op {
                    undo: Undo::Compensate(vec![]),
                    ..owned("a")
                })],
            ),
        ),
        neg(
            Code::E0208,
            "undo-not-idempotent",
            lab(),
            temp(
                "posture",
                vec![s(Op {
                    undo_idempotent: false,
                    ..owned("a")
                })],
            ),
        ),
        neg(
            Code::E0407,
            "target-undo-on-api-host",
            lab(),
            temp(
                "bmc",
                vec![s(Op {
                    locus: Locus::Host(HostRef::Static("api-01".into())),
                    ..target_undo(owned("a"))
                })],
            ),
        ),
        // Interference (section 5.7).
        neg(
            Code::E0301,
            "umbra-conflict",
            lab(),
            temp("twice", vec![s(owned("a")), s(owned("a"))]),
        ),
        neg(
            Code::E0302,
            "may-conflict-strict",
            lab(),
            temp(
                "any",
                vec![
                    s(owned("x")),
                    s(Op::new(
                        "any",
                        vec![FootprintEntry::entry(Kind::Owned, "file:/etc/{name}")],
                    )),
                ],
            ),
        ),
        // The same shape on a host bound at runtime: penumbral by host, so a
        // may-conflict (and an unresolved binding in the verdict).
        neg(
            Code::E0302,
            "may-conflict-bound-host",
            lab(),
            temp(
                "any",
                vec![
                    s(owned("a")),
                    s(Op {
                        locus: Locus::Host(HostRef::Bound("pick".into())),
                        undo_locus: UndoLocus::Controller,
                        ..owned("a")
                    }),
                ],
            ),
        ),
        neg(
            Code::E0303,
            "par-not-disjoint",
            lab(),
            temp(
                "par",
                vec![Item::Par {
                    children: vec![s(owned("a")), s(owned("a"))],
                }],
            ),
        ),
        neg(
            Code::E0304,
            "reach-inside-par",
            lab(),
            Plan {
                backstop: Some(after_1h()),
                ..temp(
                    "par",
                    vec![Item::Par {
                        children: vec![
                            s(Op {
                                reach: strings(&["ssh"]),
                                ..target_undo(owned("pf"))
                            }),
                            s(owned("b")),
                        ],
                    }],
                )
            },
        ),
        neg(
            Code::E0305,
            "anchor-twice",
            lab(),
            temp(
                "regions",
                vec![
                    s(Op::new(
                        "r1",
                        vec![FootprintEntry::anchored("file:/etc/keys", "rue")],
                    )),
                    s(Op::new(
                        "r2",
                        vec![FootprintEntry::anchored("file:/etc/keys", "rue")],
                    )),
                ],
            ),
        ),
        // Backstops (section 5.6).
        neg(
            Code::E0403,
            "scheduler-absent",
            lab(),
            Plan {
                backstop: Some(after_1h()),
                owner: "island".into(),
                ..temp("posture", vec![s(target_undo(owned("a")))])
            },
        ),
        neg(
            Code::E0405,
            "heartbeat-too-slow",
            lab(),
            Plan {
                backstop: Some(backstop(
                    vec![
                        Trigger::After(dur(3600)),
                        Trigger::UnlessHeartbeat {
                            deadline: dur(60),
                            interval: Some(dur(30)),
                        },
                    ],
                    1,
                )),
                ..temp("posture", vec![s(target_undo(owned("a")))])
            },
        ),
        neg(
            Code::E0503,
            "backstop-after-not-wane",
            lab(),
            Plan {
                backstop: Some(backstop(vec![Trigger::After(dur(7200))], 1)),
                ..temp("posture", vec![s(target_undo(owned("a")))])
            },
        ),
        neg(
            Code::E0504,
            "permanent-backstop-on-timer",
            lab(),
            Plan {
                backstop: Some(after_1h()),
                ..Plan::new(
                    "posture",
                    "db-01",
                    vec![s(target_undo(owned("a"))), Item::Commit],
                )
            },
        ),
        // A windowed step gate on a knell, satisfiable by waiting alone.
        neg(
            Code::E0509,
            "zero-human-step-gate",
            lab(),
            temp(
                "fence",
                vec![Item::Knell(StepI {
                    gate: Some(wait(1800)),
                    window: Some(dur(3600)),
                    ..StepI::new(fence_knell(Ack::NoAck("driver verified off".into())))
                })],
            ),
        ),
    ]
}
