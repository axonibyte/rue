//! The cross-plan ledger: reservation at request, refusal of overlap and of a
//! held exclusivity class, rehearsals reserving nothing, release on close.

use rue_core::interference::Fact;
use rue_core::ledger::*;

fn inst(id: &str, umbra: Vec<Fact>) -> Instance {
    Instance {
        id: id.into(),
        host: "db-01".into(),
        umbra,
        exclusivity: None,
        rehearsal: false,
    }
}

fn pf() -> Fact {
    Fact::new("file:/etc/pf.conf", None)
}

#[test]
fn a_second_plan_overlapping_a_pending_umbra_is_refused_with_r0203() {
    let l = Ledger::new().request(inst("a", vec![pf()])).unwrap();
    assert_eq!(
        l.request(inst("b", vec![pf()])).map(|_| ()).unwrap_err().0,
        LedgerCode::R0203
    );
}

#[test]
fn distinct_regions_on_one_fact_coexist_and_the_same_anchor_never_does() {
    let a = Fact::new("file:/root/.ssh/authorized_keys", Some("rue-a"));
    let b = Fact::new("file:/root/.ssh/authorized_keys", Some("rue-b"));
    let l = Ledger::new().request(inst("a", vec![a.clone()])).unwrap();
    assert!(l.request(inst("b", vec![b])).is_ok());
    // Two plans holding one anchor on one fact are the same umbra, not
    // two disjoint ones (5.2): the second is refused.
    assert_eq!(
        l.request(inst("c", vec![a])).map(|_| ()).unwrap_err().0,
        LedgerCode::R0203
    );
}

#[test]
fn a_held_exclusivity_class_refuses_with_r0101_regardless_of_umbra() {
    let l = Ledger::new()
        .request(Instance {
            exclusivity: Some("corpse-1".into()),
            ..inst("a", vec![])
        })
        .unwrap();
    assert_eq!(
        l.request(Instance {
            exclusivity: Some("corpse-1".into()),
            ..inst("b", vec![pf()])
        })
        .map(|_| ())
        .unwrap_err()
        .0,
        LedgerCode::R0101
    );
}

#[test]
fn a_rehearsal_reserves_nothing_and_is_never_blocked() {
    let l = Ledger::new().request(inst("a", vec![pf()])).unwrap();
    let l2 = l
        .request(Instance {
            rehearsal: true,
            ..inst("r", vec![pf()])
        })
        .unwrap();
    assert_eq!(l2.holdings().len(), 1);
}

#[test]
fn release_admits_what_was_refused() {
    let l = Ledger::new().request(inst("a", vec![pf()])).unwrap();
    assert!(l.release("a").request(inst("b", vec![pf()])).is_ok());
}

#[test]
fn different_hosts_never_interfere() {
    let l = Ledger::new().request(inst("a", vec![pf()])).unwrap();
    assert!(l
        .request(Instance {
            host: "db-02".into(),
            ..inst("b", vec![pf()])
        })
        .is_ok());
}
