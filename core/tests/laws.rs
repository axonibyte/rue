//! The laws of section 5.5, as properties over generated plans (a port of the
//! prototype's Test.Laws with a seeded generator in place of QuickCheck).

mod common;

use common::gen::*;
use common::{knell_op, owned, s};
use rue_core::algebra::*;
use rue_core::model::*;

const ROUNDS: u32 = 300;

#[test]
fn reverse_reverse_is_identity_on_knell_free_plans() {
    let mut rng = Rng(0x1234_5678);
    for _ in 0..ROUNDS {
        let items = gen_knell_free(&mut rng);
        let twice = reverse_items(&items).and_then(|r| reverse_items(&r));
        assert_eq!(twice, Ok(items));
    }
}

#[test]
fn reverse_seq_is_seq_of_reverses_swapped() {
    let mut rng = Rng(0x0BAD_CAFE);
    for _ in 0..ROUNDS {
        let a = gen_knell_free(&mut rng);
        let b = gen_knell_free(&mut rng);
        let lhs = reverse_items(&seq_(&a, &b));
        let rhs = reverse_items(&b).and_then(|rb| reverse_items(&a).map(|ra| seq_(&rb, &ra)));
        assert_eq!(lhs, rhs);
    }
}

#[test]
fn reverse_par_is_par_of_reverses() {
    let mut rng = Rng(0x600D_F00D);
    for _ in 0..ROUNDS {
        let xs = gen_knell_free(&mut rng);
        let lhs = reverse_item(&par_(xs.clone()));
        let rhs = xs
            .iter()
            .map(reverse_item)
            .collect::<Result<Vec<_>, _>>()
            .map(|children| Item::Par { children });
        assert_eq!(lhs, rhs);
    }
}

#[test]
fn a_plan_with_a_knell_anywhere_refuses_to_reverse() {
    let mut rng = Rng(0xDEAD_BEEF);
    for _ in 0..ROUNDS {
        let items = gen_with_knell(&mut rng);
        match reverse_items(&items) {
            Err(Refused { at }) if matches!(*at, Item::Knell(_)) => {}
            other => panic!("did not refuse: {other:?}"),
        }
    }
}

#[test]
fn reverse_from_undoes_the_first_k_leaves_in_reverse() {
    let mut rng = Rng(0x5EED_1E55);
    for _ in 0..ROUNDS {
        let items = gen_knell_free(&mut rng);
        let ls = leaves(&items);
        let k = rng.below(ls.len() as u32 + 1) as usize;
        let expected: Result<Vec<Item>, Refused> =
            ls.iter().take(k).rev().map(|it| reverse_item(it)).collect();
        assert_eq!(reverse_from(k, &items), expected);
    }
}

#[test]
fn reversing_a_step_flips_its_direction_and_nothing_else() {
    let mut rng = Rng(0x0F1E_2D3C);
    for _ in 0..ROUNDS {
        let st = gen_step(&mut rng);
        let mut flipped = st.clone();
        flipped.direction = match st.direction {
            Direction::Forward => Direction::Inverse,
            Direction::Inverse => Direction::Forward,
        };
        assert_eq!(reverse_item(&Item::Step(st)), Ok(Item::Step(flipped)));
    }
}

#[test]
fn numbering_counts_leaves_not_containers() {
    let s1 = || s(owned("a"));
    let items = vec![
        s1(),
        Item::Par {
            children: vec![s1(), s1()],
        },
        Item::Confirm,
        Item::Commit,
    ];
    let ns: Vec<u32> = numbered(&items).into_iter().map(|(n, _)| n).collect();
    assert_eq!(ns, vec![1, 2, 3, 4, 5]);
    assert!(knell_free(&items));
    assert!(!knell_free(&[Item::Repeat {
        form: RepeatForm::Count(1),
        var: "i".into(),
        body: vec![Item::Knell(StepI::new(knell_op()))]
    }]));
}
