//! Gates: satisfiability, minimum humans, the zero-human path, the requester
//! exclusion, wait-alone, and the surface spelling.

use rue_core::gates::*;
use rue_core::model::*;

fn auths() -> Vec<Authenticator> {
    vec![
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
    ]
}

fn auth(id: &str) -> Factor {
    Factor::Auth {
        id: id.into(),
        weight: 1,
    }
}

#[test]
fn minimum_distinct_humans_is_over_satisfying_paths() {
    let g = GateExpr::Thresh {
        n: 2,
        factors: vec![auth("oncall"), auth("alice"), auth("driver")],
    };
    let r = report(&auths(), &g);
    assert!(r.satisfiable);
    assert_eq!(r.min_distinct_humans, Some(1));
    assert!(!r.zero_human_path);
    assert_eq!(r.wait_alone_at, None);
}

#[test]
fn an_unknown_authenticator_or_impossible_threshold_is_unsatisfiable() {
    assert!(!report(&auths(), &GateExpr::Single(auth("nobody"))).satisfiable);
    assert_eq!(
        unknown_authenticators(&auths(), &GateExpr::Single(auth("nobody"))),
        vec!["nobody".to_string()]
    );
    assert!(unknown_authenticators(&auths(), &GateExpr::Single(auth("oncall"))).is_empty());
    let r = report(
        &auths(),
        &GateExpr::Thresh {
            n: 5,
            factors: vec![auth("oncall")],
        },
    );
    assert!(!r.satisfiable);
    assert_eq!(r.min_distinct_humans, None);
}

#[test]
fn a_wait_alone_gives_a_zero_human_path_and_its_instant() {
    let g = GateExpr::Single(Factor::Wait {
        duration: Duration::new(1800),
        weight: 1,
    });
    let r = report(&auths(), &g);
    assert!(r.zero_human_path);
    assert_eq!(r.wait_alone_at, Some(Duration::new(1800)));
    assert_eq!(r.min_distinct_humans, Some(0));
    // A non-human authenticator alone is a zero-human path without a wait.
    let r2 = report(&auths(), &GateExpr::Single(auth("driver")));
    assert!(r2.zero_human_path);
    assert_eq!(r2.wait_alone_at, None);
}

#[test]
fn wait_alone_ignores_paths_that_also_need_a_human() {
    // A human with a short wait, or a long wait alone: the wait-alone instant
    // is the long one, since the short wait never satisfies the gate by itself.
    let g = GateExpr::Thresh {
        n: 2,
        factors: vec![
            Factor::Wait {
                duration: Duration::new(1800),
                weight: 1,
            },
            auth("oncall"),
            Factor::Wait {
                duration: Duration::new(3600),
                weight: 2,
            },
        ],
    };
    let r = report(&auths(), &g);
    assert_eq!(r.min_distinct_humans, Some(0));
    assert!(r.zero_human_path);
    assert_eq!(r.wait_alone_at, Some(Duration::new(3600)));
}

#[test]
fn a_group_nests_and_humans_counts_every_human() {
    let g = GateExpr::Single(Factor::Group {
        expr: Box::new(GateExpr::Single(Factor::Humans { weight: 1 })),
        weight: 1,
    });
    let r = report(&auths(), &g);
    assert_eq!(r.min_distinct_humans, Some(1));
    assert!(!r.zero_human_path);
}

#[test]
fn the_requester_is_counted_by_name_or_by_humans() {
    assert!(counts_requester(
        &auths(),
        "requester",
        &GateExpr::Single(auth("requester"))
    ));
    assert!(counts_requester(
        &auths(),
        "requester",
        &GateExpr::Single(Factor::Humans { weight: 1 })
    ));
    assert!(!counts_requester(
        &auths(),
        "driver",
        &GateExpr::Single(Factor::Humans { weight: 1 })
    ));
    assert!(!counts_requester(
        &auths(),
        "requester",
        &GateExpr::Single(auth("oncall"))
    ));
    assert!(counts_requester(
        &auths(),
        "requester",
        &GateExpr::Thresh {
            n: 1,
            factors: vec![
                auth("oncall"),
                Factor::Group {
                    expr: Box::new(GateExpr::Single(auth("requester"))),
                    weight: 1
                }
            ]
        }
    ));
}

#[test]
fn the_surface_spelling() {
    assert_eq!(
        render_gate(&GateExpr::Single(auth("oncall"))),
        "auth(:oncall)"
    );
    assert_eq!(
        render_gate(&GateExpr::Single(Factor::Humans { weight: 1 })),
        "humans()"
    );
    assert_eq!(
        render_gate(&GateExpr::Single(Factor::Humans { weight: 2 })),
        "humans(weight: 2)"
    );
    assert_eq!(
        render_gate(&GateExpr::Single(Factor::Wait {
            duration: Duration::new(1800),
            weight: 1
        })),
        "wait(30m)"
    );
    assert_eq!(
        render_gate(&GateExpr::Thresh {
            n: 2,
            factors: vec![
                auth("oncall"),
                Factor::Auth {
                    id: "alice".into(),
                    weight: 2
                }
            ]
        }),
        "thresh(2, auth(:oncall), auth(:alice, weight: 2))"
    );
    assert_eq!(
        render_gate(&GateExpr::Single(Factor::Group {
            expr: Box::new(GateExpr::Single(Factor::Humans { weight: 1 })),
            weight: 1
        })),
        "group(humans())"
    );
}

#[test]
fn a_gate_is_satisfied_by_a_path_whose_authenticators_are_all_proved() {
    // Two of oncall, alice and driver.
    let g = GateExpr::Thresh {
        n: 2,
        factors: vec![auth("oncall"), auth("alice"), auth("driver")],
    };
    let zero = Duration::new(0);
    assert!(!satisfied(&auths(), &g, &[], zero));
    assert!(!satisfied(&auths(), &g, &["oncall".into()], zero));
    assert!(satisfied(
        &auths(),
        &g,
        &["oncall".into(), "driver".into()],
        zero
    ));
    // A proof from an authenticator the gate does not name counts for
    // nothing.
    assert!(!satisfied(
        &auths(),
        &g,
        &["oncall".into(), "requester".into()],
        zero
    ));
    // A non-human authenticator still needs its proof: weight is not
    // consent.
    assert!(!satisfied(&auths(), &g, &["driver".into()], zero));
}

#[test]
fn a_wait_factor_is_weight_that_accrues_and_says_when_it_will() {
    // One human, or a two-hour wait.
    let g = GateExpr::Thresh {
        n: 2,
        factors: vec![
            auth("oncall"),
            Factor::Wait {
                duration: Duration::new(7_200),
                weight: 2,
            },
        ],
    };
    assert!(!satisfied(&auths(), &g, &[], Duration::new(7_199)));
    assert!(satisfied(&auths(), &g, &[], Duration::new(7_200)));
    // With no proofs the gate opens by wait alone; the wait is what is
    // left to do.
    assert_eq!(
        satisfiable_at(&auths(), &g, &[]),
        Some(Duration::new(7_200))
    );
    // A gate no proof can reach says so.
    let unreachable = GateExpr::Single(auth("nobody"));
    assert_eq!(satisfiable_at(&auths(), &unreachable, &[]), None);
    assert!(!satisfied(
        &auths(),
        &unreachable,
        &["nobody".into()],
        zero()
    ));
}

fn zero() -> Duration {
    Duration::new(0)
}
