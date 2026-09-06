//! The interference query at module level: the rules the goldens depend on,
//! stated one by one.

mod common;

use common::*;
use rue_core::interference::*;
use rue_core::model::*;

fn other_host(o: Op, h: &str) -> Op {
    Op {
        locus: Locus::Host(HostRef::Static(h.into())),
        undo_locus: UndoLocus::Controller,
        ..o
    }
}

#[test]
fn the_same_owned_fact_written_twice_conflicts_once() {
    let items = vec![s(owned("a")), s(owned("a"))];
    assert_eq!(
        conflict("db-01", &items),
        vec![Conflict {
            earlier: 1,
            later: 2,
            fact: Fact::new("file:/a", None)
        }]
    );
    assert!(conflict("db-01", &[s(owned("a")), s(owned("b"))]).is_empty());
}

#[test]
fn a_runtime_bound_shape_may_conflict_with_a_static_one_sharing_its_prefix() {
    let items = vec![
        s(owned("etc/x")),
        s(Op::new(
            "any",
            vec![FootprintEntry::entry(Kind::Owned, "file:/etc/{name}")],
        )),
    ];
    assert!(conflict("db-01", &items).is_empty());
    assert_eq!(
        mayconflict("db-01", &items),
        vec![Conflict {
            earlier: 1,
            later: 2,
            fact: Fact::new("file:/etc/x", None)
        }]
    );
}

#[test]
fn par_siblings_are_never_an_ordered_pair_but_are_judged_by_par_ok() {
    let items = vec![Item::Par {
        children: vec![s(owned("a")), s(owned("a"))],
    }];
    assert!(conflict("db-01", &items).is_empty());
    assert_eq!(par_siblings("db-01", &items), vec![(1, 2)]);
    assert_eq!(par_violations("db-01", &items), (vec![(1, 2)], vec![]));
    assert!(!par_ok("db-01", &items));
    assert!(par_ok(
        "db-01",
        &[Item::Par {
            children: vec![s(owned("a")), s(owned("b"))]
        }]
    ));
}

#[test]
fn a_step_after_a_par_conflicts_with_a_par_child_in_order() {
    let items = vec![
        Item::Par {
            children: vec![s(owned("a")), s(owned("b"))],
        },
        s(owned("a")),
    ];
    assert_eq!(
        conflict("db-01", &items),
        vec![Conflict {
            earlier: 1,
            later: 3,
            fact: Fact::new("file:/a", None)
        }]
    );
}

#[test]
fn reach_inside_par_is_reported() {
    let reachy = Op {
        undo_locus: UndoLocus::Target,
        reach: vec!["ssh".into()],
        ..owned("pf")
    };
    let items = vec![Item::Par {
        children: vec![s(reachy), s(owned("b"))],
    }];
    assert_eq!(par_violations("db-01", &items).1, vec![1]);
}

#[test]
fn a_repeat_over_loop_is_disjoint_with_itself_but_not_with_the_outside() {
    // Two steps in one iteration both touch the iteration's own instance;
    // across iterations the loop variable keeps them apart, so no pair inside
    // the body may-conflicts (a body of one step would prove nothing).
    let touch = |id: &str| {
        s(Op::new(
            id,
            vec![FootprintEntry::entry(Kind::Modified, "guest:{g}:state")],
        ))
    };
    let lp = Item::Repeat {
        form: RepeatForm::Over {
            list: "guests".into(),
            max: 8,
            set_valued: true,
        },
        var: "g".into(),
        body: vec![touch("stop"), touch("start")],
    };
    assert!(mayconflict("db-01", std::slice::from_ref(&lp)).is_empty());
    // A counted repeat binds no variable: the same two steps may-conflict.
    let counted = Item::Repeat {
        form: RepeatForm::Count(2),
        var: "i".into(),
        body: vec![touch("stop"), touch("start")],
    };
    assert_eq!(
        mayconflict("db-01", std::slice::from_ref(&counted)).len(),
        1
    );
    let outside = s(Op::new(
        "touch",
        vec![FootprintEntry::entry(Kind::Modified, "guest:x:state")],
    ));
    // Outside the loop the variable is unbound to it: both body steps may
    // conflict with a later write to a guest.
    assert_eq!(mayconflict("db-01", &[lp, outside]).len(), 2);
}

#[test]
fn distinct_anchors_on_one_fact_are_disjoint_and_a_repeated_anchor_is_found() {
    let r = |id: &str, anchor: &str| {
        s(Op::new(
            id,
            vec![FootprintEntry::anchored("file:/etc/keys", anchor)],
        ))
    };
    assert!(conflict("db-01", &[r("r1", "a"), r("r2", "b")]).is_empty());
    assert!(anchor_duplicates("db-01", &[r("r1", "a"), r("r2", "b")]).is_empty());
    let dup = vec![r("r1", "rue"), r("r2", "rue")];
    assert_eq!(
        anchor_duplicates("db-01", &dup),
        vec![Conflict {
            earlier: 1,
            later: 2,
            fact: Fact::new("file:/etc/keys", Some("rue"))
        }]
    );
    // The repeated anchor is also, formally, a conflict; the checker prefers E0305.
    assert_eq!(conflict("db-01", &dup).len(), 1);
}

#[test]
fn the_same_shape_on_two_static_hosts_is_two_facts() {
    let items = vec![s(owned("a")), s(other_host(owned("a"), "api-01"))];
    assert!(conflict("db-01", &items).is_empty());
    assert!(mayconflict("db-01", &items).is_empty());
    assert!(par_ok("db-01", &[Item::Par { children: items }]));
}

#[test]
fn a_host_bound_at_runtime_makes_its_facts_penumbral() {
    let bound = Op {
        locus: Locus::Host(HostRef::Bound("pick".into())),
        undo_locus: UndoLocus::Controller,
        ..owned("a")
    };
    let items = vec![s(owned("a")), s(bound.clone())];
    assert!(conflict("db-01", &items).is_empty());
    assert_eq!(mayconflict("db-01", &items).len(), 1);
    assert_eq!(leaf_host_text("db-01", &bound), "{pick}");
    assert_eq!(leaf_host_text("db-01", &owned("a")), "db-01");
    assert_eq!(
        leaf_host_text(
            "db-01",
            &Op {
                locus: Locus::Controller,
                ..owned("a")
            }
        ),
        "controller"
    );
}

#[test]
fn a_controller_step_and_a_target_step_never_share_a_fact() {
    let items = vec![
        s(owned("a")),
        s(Op {
            locus: Locus::Controller,
            ..owned("a")
        }),
    ];
    assert!(conflict("db-01", &items).is_empty());
}

#[test]
fn needs_follows_the_undo_form() {
    let o = owned("a");
    assert_eq!(needs(&o), vec![Fact::new("file:/a", None)]);
    assert_eq!(
        needs(&Op {
            undo: Undo::Compensate(vec!["x".into()]),
            ..o.clone()
        }),
        vec![Fact::new("x", None)]
    );
    assert!(needs(&Op {
        undo: Undo::NoUndo,
        ..o.clone()
    })
    .is_empty());
    assert!(writes(&Op::new(
        "d",
        vec![FootprintEntry::entry(Kind::Derived, "probe:p")]
    ))
    .is_empty());
    assert_eq!(
        maywrite(&Op::new(
            "m",
            vec![FootprintEntry::entry(Kind::Held, "proc:{n}")]
        ))
        .len(),
        1
    );
}

#[test]
fn leaves_are_numbered_through_containers_and_carry_loop_variables() {
    let inner = s(Op::new(
        "stop",
        vec![FootprintEntry::entry(Kind::Modified, "guest:{g}:state")],
    ));
    let items = vec![
        Item::Confirm,
        Item::Repeat {
            form: RepeatForm::Over {
                list: "guests".into(),
                max: 8,
                set_valued: true,
            },
            var: "g".into(),
            body: vec![inner],
        },
        s(owned("z")),
    ];
    let leaves = step_facts("db-01", &items);
    let ns: Vec<u32> = leaves.iter().map(|l| l.n).collect();
    assert_eq!(ns, vec![2, 3]);
    assert_eq!(leaves[0].vars, vec!["g".to_string()]);
    assert!(leaves[1].vars.is_empty());
}
