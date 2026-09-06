//! Tier 4: the five class rules of section 5.9, asserted over every context,
//! state and event the machine admits. The table itself is a golden.

use rue_core::intent::Intent;
use rue_core::model::{Mode, OnLapse};
use rue_core::states::*;

fn manual_temporary() -> Ctx {
    Ctx {
        intent: Intent::Temporary,
        mode: Mode::Manual,
        earlier_hold: false,
        on_lapse: OnLapse::Revert,
    }
}

#[test]
fn rule_1_terminal_states_admit_no_event() {
    for c in all_ctxs() {
        for s in [State::Closed, State::Committed] {
            for &e in ALL_EVENTS {
                assert_eq!(
                    transition(c, s, e),
                    Outcome::NotApplicable,
                    "{s} moved on {e}"
                );
            }
        }
    }
}

#[test]
fn rule_2_wane_expires_every_bounded_state_of_a_temporary_plan() {
    let exempt = [
        State::DriftHeld,
        State::Stuck,
        State::Pending,
        State::Unchecked,
        State::Checked,
        State::ApprovalExpired,
        State::Expired,
    ];
    for c in all_ctxs()
        .into_iter()
        .filter(|c| c.intent == Intent::Temporary)
    {
        for &s in ALL_STATES {
            if terminal(s) || exempt.contains(&s) {
                continue;
            }
            assert_eq!(
                transition(c, s, Event::WaneElapses),
                Outcome::To(State::Expired),
                "{s} survived wane"
            );
        }
    }
}

#[test]
fn rule_2_wane_never_holds_and_a_permanent_plan_has_no_wane_event() {
    for c in all_ctxs() {
        for &s in ALL_STATES {
            assert_ne!(
                transition(c, s, Event::WaneElapses),
                Outcome::To(State::Held)
            );
            if c.intent == Intent::Permanent {
                assert_eq!(
                    transition(c, s, Event::WaneElapses),
                    Outcome::NotApplicable,
                    "wane occurred on a permanent plan in {s}"
                );
            }
        }
    }
}

#[test]
fn rule_3_drift_held_and_stuck_outlive_wane_and_only_a_human_ends_them() {
    for c in all_ctxs() {
        for s in [State::DriftHeld, State::Stuck] {
            assert!(matches!(
                transition(c, s, Event::WaneElapses),
                Outcome::Stay | Outcome::NotApplicable
            ));
        }
        for &e in ALL_EVENTS {
            assert!(matches!(
                transition(c, State::DriftHeld, e),
                Outcome::To(State::Reverting)
                    | Outcome::To(State::Closed)
                    | Outcome::Stay
                    | Outcome::NotApplicable
                    | Outcome::Refuse(RCode::R0103)
            ));
            assert!(matches!(
                transition(c, State::Stuck, e),
                Outcome::To(State::Reverting)
                    | Outcome::To(State::Closed)
                    | Outcome::Stay
                    | Outcome::NotApplicable
            ));
        }
    }
}

#[test]
fn rule_3_held_and_deferred_in_a_permanent_plan_wait_for_an_operator() {
    for c in all_ctxs()
        .into_iter()
        .filter(|c| c.intent == Intent::Permanent)
    {
        for s in [State::Held, State::Deferred] {
            for e in [Event::WaneElapses, Event::BoundLapses] {
                assert_eq!(
                    transition(c, s, e),
                    Outcome::NotApplicable,
                    "a permanent hold ended by time"
                );
            }
        }
    }
}

#[test]
fn rule_4_refusal_reverts_unless_an_earlier_step_holds_and_lapses_follow_on_lapse() {
    for c in all_ctxs() {
        let expected = if c.earlier_hold {
            Outcome::To(State::Held)
        } else {
            Outcome::To(State::Reverting)
        };
        assert_eq!(transition(c, State::Applying, Event::Refuse), expected);
        let lapse = if c.mode == Mode::Auto || c.on_lapse == OnLapse::Revert {
            Outcome::To(State::Reverting)
        } else {
            Outcome::To(State::Held)
        };
        assert_eq!(transition(c, State::Waiting, Event::BoundLapses), lapse);
    }
}

#[test]
fn rule_5_commit_renew_and_confirm_follow_intent() {
    for c in all_ctxs() {
        let permanent = c.intent == Intent::Permanent;
        for (s, e) in [
            (State::Applying, Event::CommitItem),
            (State::Held, Event::CommitVerb),
            (State::Deferred, Event::CommitVerb),
        ] {
            let expected = if permanent {
                Outcome::To(State::Committed)
            } else {
                Outcome::Refuse(RCode::R0102)
            };
            assert_eq!(transition(c, s, e), expected, "{s} {e}");
        }
        assert_eq!(
            transition(c, State::Applying, Event::Renew),
            if permanent {
                Outcome::Refuse(RCode::R0102)
            } else {
                Outcome::Stay
            }
        );
        assert_eq!(
            transition(c, State::Applying, Event::Confirm),
            if permanent {
                Outcome::Stay
            } else {
                Outcome::Refuse(RCode::R0102)
            }
        );
        if !permanent {
            assert_eq!(transition(c, State::Applied, Event::Renew), Outcome::Stay);
        }
    }
}

#[test]
fn a_permanent_plan_has_no_applied_or_suspended_state() {
    for c in all_ctxs()
        .into_iter()
        .filter(|c| c.intent == Intent::Permanent)
    {
        for s in [State::Applied, State::Suspended] {
            for &e in ALL_EVENTS {
                assert_eq!(transition(c, s, e), Outcome::NotApplicable);
            }
        }
    }
}

#[test]
fn recant_on_drift_held_without_force_is_r0103_and_abandon_closes_only_stuck_and_drift_held() {
    assert_eq!(
        transition(manual_temporary(), State::DriftHeld, Event::Recant),
        Outcome::Refuse(RCode::R0103)
    );
    assert_eq!(
        transition(manual_temporary(), State::DriftHeld, Event::ForceDrift),
        Outcome::To(State::Reverting)
    );
    for c in all_ctxs() {
        for &s in ALL_STATES {
            let expected = if matches!(s, State::Stuck | State::DriftHeld) {
                Outcome::To(State::Closed)
            } else {
                Outcome::NotApplicable
            };
            assert_eq!(transition(c, s, Event::Abandon), expected);
        }
    }
}

#[test]
fn the_table_lists_only_applicable_transitions_and_respects_intent() {
    for (c, _, _, o) in transition_table() {
        assert_ne!(o, Outcome::NotApplicable);
        if c.intent == Intent::Temporary {
            assert_ne!(o, Outcome::To(State::Committed));
        } else {
            assert_ne!(o, Outcome::To(State::Applied));
        }
    }
    let table = render_table();
    assert!(table.starts_with("intent\tmode\tearlier_hold\ton_lapse\tstate\tevent\toutcome\n"));
    assert_eq!(table.lines().count(), transition_table().len() + 1);
    assert!(table.contains("temporary\tmanual\tno\trevert\tUnchecked\tCheck\t-> Checked\n"));
    assert!(table.contains("\tDriftHeld\tRecant\trefuse R0103\n"));
}

mod time {
    use rue_core::model::{Duration, Instant};
    use rue_core::states::{expired, renew, RenewRefusal};

    #[test]
    fn expiry_is_observed_at_the_instant_closed_boundary() {
        let deadline = Instant::new(1000);
        assert!(!expired(Instant::new(999), deadline));
        assert!(
            expired(Instant::new(1000), deadline),
            "observed at the instant is expired"
        );
        assert!(expired(Instant::new(1001), deadline));
    }

    #[test]
    fn renewal_is_within_the_window_never_after_expiry_and_anchored_at_renewal() {
        let deadline = Instant::new(1000);
        let within = Duration::new(300);
        let wane = Duration::new(3600);
        assert_eq!(
            renew(Instant::new(1000), deadline, within, wane),
            Err(RenewRefusal::Expired)
        );
        assert_eq!(
            renew(Instant::new(1500), deadline, within, wane),
            Err(RenewRefusal::Expired)
        );
        assert_eq!(
            renew(Instant::new(699), deadline, within, wane),
            Err(RenewRefusal::OutsideWindow {
                until: Instant::new(700)
            })
        );
        assert_eq!(
            renew(Instant::new(700), deadline, within, wane),
            Ok(Instant::new(4300))
        );
        assert_eq!(
            renew(Instant::new(999), deadline, within, wane),
            Ok(Instant::new(4599))
        );
    }
}
