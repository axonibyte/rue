//! The runtime state machine, docs/ROADMAP.md section 5.9, derived from its
//! five class rules rather than drawn:
//!
//! 1. Terminal: Closed, Committed.
//! 2. Bounded by wane in a temporary plan: every non-terminal state except
//!    DriftHeld and Stuck; Pending's bound is its approval window. Wane
//!    elapsing is always Expired then Reverting, never a hold.
//! 3. Unbounded, by declaration: DriftHeld and Stuck in any plan; Held and
//!    Deferred in a permanent plan. A permanent plan's Waiting is bounded by
//!    its window or the site's max_wait.
//! 4. Refusal during Applying goes to Reverting, unless an earlier applied
//!    step has refusal :hold, in which case to Held. A window or max_wait
//!    lapse before wane resolves per on_lapse (revert default; hold; always
//!    revert under auto).
//! 5. Commit is reached from Applying by the item, from Held or Deferred by
//!    the verb; commit, renew and confirm on a plan whose intent does not
//!    admit them are R0102.
//!
//! The table `rue states` prints and the tier-4 truth-table test are both
//! generated from [`transition`].

use std::fmt;

use crate::intent::Intent;
use crate::model::{Duration, Instant, Mode, OnLapse};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    Unchecked,
    Checked,
    Pending,
    ApprovalExpired,
    Applying,
    Waiting,
    Deferred,
    Held,
    Applied,
    Suspended,
    Expired,
    Reverting,
    Stuck,
    DriftHeld,
    Committed,
    Closed,
}

pub const ALL_STATES: &[State] = &[
    State::Unchecked,
    State::Checked,
    State::Pending,
    State::ApprovalExpired,
    State::Applying,
    State::Waiting,
    State::Deferred,
    State::Held,
    State::Applied,
    State::Suspended,
    State::Expired,
    State::Reverting,
    State::Stuck,
    State::DriftHeld,
    State::Committed,
    State::Closed,
];

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Event {
    Check,
    Request,
    Approve,
    ApprovalWindowLapses,
    Cancel,
    HostContractChanged,
    AllStepsDone,
    CommitItem,
    Refuse,
    /// A gate, ack or unknown guard at a step.
    WaitAtStep,
    /// Satisfied, acked or forced.
    WaitSatisfied,
    DeferAtStep,
    HandoffDone,
    Recant,
    Suspend,
    Reestablish,
    Renew,
    Confirm,
    CommitVerb,
    Resume,
    /// The step's window or the site's max_wait, before wane.
    BoundLapses,
    WaneElapses,
    UndoClean,
    UndoFailed,
    Retry,
    DriftOnDefer,
    ForceDrift,
    Abandon,
}

pub const ALL_EVENTS: &[Event] = &[
    Event::Check,
    Event::Request,
    Event::Approve,
    Event::ApprovalWindowLapses,
    Event::Cancel,
    Event::HostContractChanged,
    Event::AllStepsDone,
    Event::CommitItem,
    Event::Refuse,
    Event::WaitAtStep,
    Event::WaitSatisfied,
    Event::DeferAtStep,
    Event::HandoffDone,
    Event::Recant,
    Event::Suspend,
    Event::Reestablish,
    Event::Renew,
    Event::Confirm,
    Event::CommitVerb,
    Event::Resume,
    Event::BoundLapses,
    Event::WaneElapses,
    Event::UndoClean,
    Event::UndoFailed,
    Event::Retry,
    Event::DriftOnDefer,
    Event::ForceDrift,
    Event::Abandon,
];

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Runtime codes from Appendix D that the machine can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RCode {
    /// Verb not admitted by the plan's intent.
    R0102,
    /// Recant on DriftHeld without --force=drift.
    R0103,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    To(State),
    /// The event is observed and the state is unchanged.
    Stay,
    /// The event is refused with a code; the state is unchanged.
    Refuse(RCode),
    /// The event cannot occur in this state.
    NotApplicable,
}

/// What a transition may depend on besides the state and the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ctx {
    pub intent: Intent,
    pub mode: Mode,
    /// An earlier applied step has refusal :hold.
    pub earlier_hold: bool,
    pub on_lapse: OnLapse,
}

pub fn terminal(s: State) -> bool {
    matches!(s, State::Closed | State::Committed)
}

pub fn transition(ctx: Ctx, s: State, ev: Event) -> Outcome {
    use Event as E;
    use Outcome::*;
    use State as S;
    let temporary = ctx.intent == Intent::Temporary;
    let permanent = ctx.intent == Intent::Permanent;
    if terminal(s) {
        return NotApplicable; // rule 1
    }
    // Applied and Suspended are temporary-plan states: a permanent plan goes
    // from Applying to Committed and never rests as Applied.
    if permanent && matches!(s, S::Applied | S::Suspended) {
        return NotApplicable;
    }
    if ev == E::WaneElapses {
        // Rules 2 and 3: wane bounds every non-terminal state of a temporary
        // plan except DriftHeld and Stuck (unbounded by declaration) and
        // Pending (bounded by its approval window instead). A permanent plan
        // has no wane, so the event cannot occur.
        return if permanent {
            NotApplicable
        } else if matches!(s, S::DriftHeld | S::Stuck) {
            Stay
        } else if matches!(
            s,
            S::Pending | S::Unchecked | S::Checked | S::ApprovalExpired
        ) {
            NotApplicable
        } else if s == S::Expired {
            Stay
        } else {
            To(S::Expired)
        };
    }
    if ev == E::Abandon {
        return if matches!(s, S::Stuck | S::DriftHeld) {
            To(S::Closed)
        } else {
            NotApplicable
        };
    }
    // Rule 4: a lapse before wane resolves per on_lapse; always revert under
    // auto. Held in a permanent plan is unbounded (rule 3), so a lapse into
    // Held is a hold until an operator acts.
    let lapse = if ctx.mode == Mode::Auto {
        To(S::Reverting)
    } else if ctx.on_lapse == OnLapse::Hold {
        To(S::Held)
    } else {
        To(S::Reverting)
    };
    let permanent_only = |o: Outcome| if permanent { o } else { Refuse(RCode::R0102) };
    let temporary_only = |o: Outcome| if temporary { o } else { Refuse(RCode::R0102) };
    match (s, ev) {
        (S::Unchecked, E::Check) => To(S::Checked),
        (S::Checked, E::Request) => To(S::Pending),
        (S::Pending, E::Approve) => To(S::Applying),
        (S::Pending, E::ApprovalWindowLapses) => To(S::ApprovalExpired),
        (S::Pending, E::Cancel) => To(S::Closed),
        (S::Pending, E::HostContractChanged) => To(S::Closed),
        (S::ApprovalExpired, E::Cancel) => To(S::Closed), // reaped
        (S::Applying, E::AllStepsDone) => {
            if temporary {
                To(S::Applied)
            } else {
                NotApplicable
            }
        }
        (S::Applying, E::CommitItem) => permanent_only(To(S::Committed)),
        (S::Applying, E::Refuse) => {
            if ctx.earlier_hold {
                To(S::Held)
            } else {
                To(S::Reverting)
            }
        } // rule 4
        (S::Applying, E::WaitAtStep) => To(S::Waiting),
        (S::Applying, E::DeferAtStep) => To(S::Deferred),
        (S::Applying, E::Confirm) => permanent_only(Stay),
        // Wane is anchored at approval, so renewal is meaningful while applying.
        (S::Applying, E::Renew) => temporary_only(Stay),
        (S::Waiting, E::WaitSatisfied) => To(S::Applying),
        (S::Waiting, E::Recant) => To(S::Reverting),
        (S::Waiting, E::BoundLapses) => lapse, // rule 4
        (S::Deferred, E::HandoffDone) => To(S::Applying),
        (S::Deferred, E::Recant) => To(S::Reverting),
        (S::Deferred, E::CommitVerb) => permanent_only(To(S::Committed)), // rule 5
        (S::Held, E::Resume) => To(S::Applying),
        (S::Held, E::Recant) => To(S::Reverting),
        (S::Held, E::CommitVerb) => permanent_only(To(S::Committed)), // rule 5
        (S::Applied, E::Recant) => To(S::Reverting),
        (S::Applied, E::Suspend) => To(S::Suspended),
        (S::Applied, E::Renew) => temporary_only(Stay),
        (S::Applied, E::Confirm) => permanent_only(Stay),
        (S::Suspended, E::Reestablish) => To(S::Applied),
        (S::Suspended, E::Recant) => To(S::Reverting),
        // Expired reverts: same exits as Reverting.
        (S::Expired, E::UndoClean) => To(S::Closed),
        (S::Expired, E::UndoFailed) => To(S::Stuck),
        (S::Expired, E::DriftOnDefer) => To(S::DriftHeld),
        (S::Reverting, E::UndoClean) => To(S::Closed),
        (S::Reverting, E::UndoFailed) => To(S::Stuck),
        (S::Reverting, E::DriftOnDefer) => To(S::DriftHeld),
        (S::Stuck, E::Retry) => To(S::Reverting),
        (S::DriftHeld, E::ForceDrift) => To(S::Reverting),
        (S::DriftHeld, E::Recant) => Refuse(RCode::R0103),
        _ => NotApplicable,
    }
}

/// Every context: both intents, both modes, both hold flags, both lapse
/// policies, in the prototype's order.
pub fn all_ctxs() -> Vec<Ctx> {
    let mut v = Vec::new();
    for intent in [Intent::Temporary, Intent::Permanent] {
        for mode in [Mode::Manual, Mode::Auto] {
            for earlier_hold in [false, true] {
                for on_lapse in [OnLapse::Revert, OnLapse::Hold] {
                    v.push(Ctx {
                        intent,
                        mode,
                        earlier_hold,
                        on_lapse,
                    });
                }
            }
        }
    }
    v
}

/// Every (context, state, event) with its outcome, applicable ones only.
pub fn transition_table() -> Vec<(Ctx, State, Event, Outcome)> {
    let mut v = Vec::new();
    for ctx in all_ctxs() {
        for &s in ALL_STATES {
            for &ev in ALL_EVENTS {
                let o = transition(ctx, s, ev);
                if o != Outcome::NotApplicable {
                    v.push((ctx, s, ev, o));
                }
            }
        }
    }
    v
}

/// The table as tab-separated text: intent, mode, earlier_hold, on_lapse,
/// state, event, outcome. Only applicable transitions are listed; a pair
/// absent from the table cannot occur.
pub fn render_table() -> String {
    let mut out = String::from("intent\tmode\tearlier_hold\ton_lapse\tstate\tevent\toutcome\n");
    for (c, s, e, o) in transition_table() {
        let outcome = match o {
            Outcome::To(t) => format!("-> {t}"),
            Outcome::Stay => "stay".to_string(),
            Outcome::Refuse(code) => format!("refuse {code:?}"),
            Outcome::NotApplicable => "n/a".to_string(),
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{s}\t{e}\t{outcome}\n",
            match c.intent {
                Intent::Temporary => "temporary",
                Intent::Permanent => "permanent",
            },
            match c.mode {
                Mode::Manual => "manual",
                Mode::Auto => "auto",
            },
            if c.earlier_hold { "yes" } else { "no" },
            match c.on_lapse {
                OnLapse::Revert => "revert",
                OnLapse::Hold => "hold",
            },
        ));
    }
    out
}

/// Whether a bound has been reached. The boundary is closed: observed *at*
/// the instant is expired (section 5.9).
pub fn expired(now: Instant, deadline: Instant) -> bool {
    now >= deadline
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenewRefusal {
    /// The plan has expired; an expired plan is never renewed.
    Expired,
    /// Renewal is accepted only within `renew_within` of expiry; the instant
    /// it opens.
    OutsideWindow { until: Instant },
}

/// Renewal of a temporary plan (section 5.6): accepted only within
/// `renew_within` of the deadline, never for an expired plan, and anchored
/// at the renewal, so the new deadline is `now + wane`.
pub fn renew(
    now: Instant,
    deadline: Instant,
    renew_within: Duration,
    wane: Duration,
) -> Result<Instant, RenewRefusal> {
    if expired(now, deadline) {
        return Err(RenewRefusal::Expired);
    }
    let opens = Instant::new(deadline.unix_s.saturating_sub(renew_within.seconds));
    if now < opens {
        return Err(RenewRefusal::OutsideWindow { until: opens });
    }
    Ok(now.plus(wane))
}
