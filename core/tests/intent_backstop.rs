//! Intent inference and its checks; backstop coverage, arming, the reach
//! rule, triggers by intent, the heartbeat bound.

mod common;

use common::*;
use rue_core::backstop::*;
use rue_core::intent::*;
use rue_core::model::*;

fn target(o: Op) -> Op {
    Op {
        undo_locus: UndoLocus::Target,
        ..o
    }
}

fn after_1h() -> Backstop {
    Backstop {
        triggers: vec![Trigger::After(Duration::new(3600))],
        arm_before: 1,
    }
}

#[test]
fn intent_is_wane_xor_reachable_commit() {
    assert_eq!(
        infer_intent(&temp(vec![s(owned("a"))])),
        Some(Intent::Temporary)
    );
    assert_eq!(
        infer_intent(&Plan::new("p", "db-01", vec![s(owned("a")), Item::Commit])),
        Some(Intent::Permanent)
    );
    assert_eq!(infer_intent(&temp(vec![s(owned("a")), Item::Commit])), None);
    assert_eq!(
        infer_intent(&Plan::new("p", "db-01", vec![s(owned("a"))])),
        None
    );
    let nested = Plan::new(
        "p",
        "db-01",
        vec![Item::When {
            guard: Guard::new("g", Tri::Yes),
            window: None,
            on_lapse: OnLapse::Revert,
            then_: vec![Item::Commit],
            else_: vec![],
        }],
    );
    assert_eq!(infer_intent(&nested), Some(Intent::Permanent));
    assert_eq!(commit_step(&nested.body), Some(1));
    assert_eq!(commit_step(&[s(owned("a"))]), None);
}

#[test]
fn fires_by_construction_is_temporary_with_the_confirmed_duration_as_wane() {
    let p = Plan {
        fires_by_construction: true,
        backstop: Some(Backstop {
            triggers: vec![Trigger::UnlessConfirmed(Duration::new(600))],
            arm_before: 1,
        }),
        ..Plan::new("p", "db-01", vec![s(target(owned("a")))])
    };
    assert_eq!(infer_intent(&p), Some(Intent::Temporary));
    assert_eq!(effective_wane(&p), Some(Duration::new(600)));
    assert_eq!(
        infer_intent(&Plan {
            wane: Some(Duration::new(1)),
            ..p.clone()
        }),
        None
    );
    assert_eq!(
        effective_wane(&Plan {
            backstop: None,
            ..p
        }),
        None
    );
}

#[test]
fn commit_must_be_last_on_its_path_and_every_path_must_reach_it() {
    assert!(commit_not_last(&[Item::Commit, s(owned("a"))]));
    assert!(!commit_not_last(&[s(owned("a")), Item::Commit]));
    let branch = |then_: Vec<Item>, else_: Vec<Item>| Item::When {
        guard: Guard::new("g", Tri::Yes),
        window: None,
        on_lapse: OnLapse::Revert,
        then_,
        else_,
    };
    assert_eq!(
        paths_without_commit(&[branch(vec![Item::Commit], vec![])]),
        1
    );
    assert_eq!(
        paths_without_commit(&[branch(vec![Item::Commit], vec![Item::Commit])]),
        0
    );
    assert_eq!(
        paths(&[
            branch(vec![s(owned("a"))], vec![s(owned("b")), s(owned("c"))]),
            Item::Commit
        ])
        .len(),
        2
    );
}

#[test]
fn coverage_is_the_target_undo_steps_with_the_late_arming_window() {
    let p = Plan {
        backstop: Some(Backstop {
            triggers: vec![Trigger::After(Duration::new(3600))],
            arm_before: 3,
        }),
        ..temp(vec![
            s(target(owned("a"))),
            s(target(owned("b"))),
            s(owned("c")),
        ])
    };
    assert_eq!(
        coverage(&p),
        Some(Coverage {
            covered: vec![1, 2],
            installed_before: Some(1),
            armed_before_step: 3,
            late_arming_window: vec![1, 2]
        })
    );
    // Armed before step 2: only step 1 completes before arming.
    let p2 = Plan {
        backstop: Some(Backstop {
            arm_before: 2,
            ..after_1h()
        }),
        ..temp(vec![s(target(owned("a"))), s(target(owned("b")))])
    };
    assert_eq!(coverage(&p2).unwrap().late_arming_window, vec![1]);
    assert_eq!(coverage(&temp(vec![])), None);
}

#[test]
fn the_reach_rule() {
    let reachy = Op {
        reach: vec!["ssh".into()],
        ..target(owned("pf"))
    };
    assert_eq!(reach_violations(&temp(vec![s(reachy.clone())])), vec![1]);
    assert!(reach_violations(&Plan {
        backstop: Some(after_1h()),
        ..temp(vec![s(reachy.clone())])
    })
    .is_empty());
    let late = Plan {
        backstop: Some(Backstop {
            arm_before: 3,
            ..after_1h()
        }),
        ..temp(vec![s(target(owned("a"))), s(reachy.clone())])
    };
    assert_eq!(reach_violations(&late), vec![2]);
    let controller_undo = Op {
        undo_locus: UndoLocus::Controller,
        ..reachy
    };
    assert_eq!(
        reach_violations(&Plan {
            backstop: Some(after_1h()),
            ..temp(vec![s(controller_undo)])
        }),
        vec![1]
    );
}

#[test]
fn triggers_follow_intent() {
    let one = |b: Backstop| Plan {
        backstop: Some(b),
        ..temp(vec![s(target(owned("a")))])
    };
    assert!(trigger_violations(Intent::Temporary, &one(after_1h())).is_empty());
    assert_eq!(
        trigger_violations(
            Intent::Temporary,
            &one(Backstop {
                triggers: vec![Trigger::After(Duration::new(7200))],
                arm_before: 1
            })
        ),
        vec![TriggerViolation::AfterNotWane]
    );
    assert_eq!(
        trigger_violations(
            Intent::Temporary,
            &one(Backstop {
                triggers: vec![
                    Trigger::After(Duration::new(3600)),
                    Trigger::UnlessConfirmed(Duration::new(600))
                ],
                arm_before: 1
            })
        ),
        vec![TriggerViolation::TemporaryConfirmed]
    );
    assert_eq!(
        trigger_violations(
            Intent::Temporary,
            &one(Backstop {
                triggers: vec![Trigger::UnlessHeartbeat {
                    deadline: Duration::new(60),
                    interval: None
                }],
                arm_before: 1
            })
        ),
        vec![TriggerViolation::AfterNotWane]
    );
    let perm = |b: Backstop, body: Vec<Item>| Plan {
        backstop: Some(b),
        ..Plan::new("p", "db-01", body)
    };
    assert_eq!(
        trigger_violations(
            Intent::Permanent,
            &perm(after_1h(), vec![s(target(owned("a"))), Item::Commit])
        ),
        vec![TriggerViolation::PermanentAfter]
    );
    let confirmed = Backstop {
        triggers: vec![Trigger::UnlessConfirmed(Duration::new(600))],
        arm_before: 1,
    };
    assert!(trigger_violations(
        Intent::Permanent,
        &perm(
            confirmed.clone(),
            vec![s(target(owned("a"))), Item::Confirm, Item::Commit]
        )
    )
    .is_empty());
    let branch = Item::When {
        guard: Guard::new("g", Tri::Yes),
        window: None,
        on_lapse: OnLapse::Revert,
        then_: vec![Item::Commit],
        else_: vec![],
    };
    assert_eq!(
        trigger_violations(
            Intent::Permanent,
            &perm(confirmed, vec![s(target(owned("a"))), branch])
        ),
        vec![TriggerViolation::NoConfirmOrCommitPath(1)]
    );
}

#[test]
fn a_heartbeat_interval_above_a_third_of_its_deadline_is_a_violation() {
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
    assert_eq!(
        heartbeat_violations(&Plan {
            backstop: Some(hb(30)),
            ..temp(vec![])
        })
        .len(),
        1
    );
    assert!(heartbeat_violations(&Plan {
        backstop: Some(hb(20)),
        ..temp(vec![])
    })
    .is_empty());
    assert!(heartbeat_violations(&temp(vec![])).is_empty());
}
