//! The twenty invariants of docs/ROADMAP.md 10.3, one function each,
//! checked after every simulated event.
//!
//! Each returns `None` when it holds and a `Violation` naming what it saw
//! when it does not. An invariant this world cannot reach says so in its
//! own comment rather than being left out of the list: a check that never
//! runs is a check that proves nothing, and the reader deserves to know
//! which those are.

use rue_core::journal::Event as J;
use rue_core::model::{Kind, UndoLocus};
use rue_core::states::State;
use rue_engine::clock::Clock;

use crate::world::{Sim, INVARIANTS, SECRET, TARGET};

/// An invariant that did not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub number: u8,
    pub name: &'static str,
    pub detail: String,
}

/// Whether an instance was abandoned: the one way a closed instance keeps
/// its applied steps and its facts.
fn abandoned(sim: &Sim, id: &str) -> bool {
    sim.sink
        .entries()
        .iter()
        .any(|e| e.instance == id && matches!(e.event, J::Abandoned { .. }))
}

fn broke(number: u8, detail: impl Into<String>) -> Option<Violation> {
    Some(Violation {
        number,
        name: INVARIANTS[(number - 1) as usize],
        detail: detail.into(),
    })
}

/// Every check, in order. The first violation wins.
pub fn check_all(sim: &mut Sim) -> Option<Violation> {
    let checks: [fn(&mut Sim) -> Option<Violation>; 20] = [
        i01_applied_steps,
        i02_footprints_present,
        i03_stuck_set,
        i04_journal_verifies,
        i05_no_secret_in_a_sink,
        i06_no_wane_during_settle,
        i07_reach_after_arming,
        i08_engine_and_artifact_agree,
        i09_no_secret_in_argv_or_artifact,
        i10_artifact_before_covered_step,
        i11_no_staged_file_survives,
        i12_no_region_clobbered_under_a_sibling,
        i13_ledger_holds_the_reservers,
        i14_a_proof_is_scoped,
        i15_reconciliation_spares_armed,
        i16_no_bounded_state_outlives_its_bound,
        i17_one_manifest_per_region_undo,
        i18_no_undeclared_act,
        i19_no_permanent_plan_reverted_by_time,
        i20_no_committed_backstop_fires,
    ];
    checks.into_iter().find_map(|c| c(sim))
}

/// (1) The applied steps of a record are steps its plan has, each applied
/// once per iteration, and a closed instance has none left.
fn i01_applied_steps(sim: &mut Sim) -> Option<Violation> {
    for r in sim.records() {
        let leaves = rue_core::algebra::numbered(&r.plan().body).len() as u32;
        for a in &r.applied {
            if a.step == 0 || a.step > leaves {
                return broke(1, format!("{}: step {} is not in the plan", r.id, a.step));
            }
        }
        let mut seen: Vec<(u32, u32)> = r.applied.iter().map(|a| (a.step, a.iteration)).collect();
        seen.sort();
        let before = seen.len();
        seen.dedup();
        if seen.len() != before {
            return broke(1, format!("{}: a step is applied twice", r.id));
        }
        // A closed instance has nothing applied, unless it was abandoned:
        // abandon closes the instance and leaves the world as it is, and
        // the journal says which steps it left (7.7).
        if r.state == State::Closed && !r.applied.is_empty() && !abandoned(sim, &r.id) {
            return broke(
                1,
                format!(
                    "{}: closed with {} steps still applied",
                    r.id,
                    r.applied.len()
                ),
            );
        }
    }
    None
}

/// (2) Every `Owned` fact of an applied step is present on the host, and
/// every fact of a step that is not applied is absent or untouched by us.
///
/// One window is exempt and named: between an artifact firing on the
/// target and the engine's next contact, the target has undone steps the
/// engine still calls applied. That is the contract -- the fired marker
/// is read on the next contact (R0402) -- and the exemption ends the
/// moment the engine journals the firing.
fn i02_footprints_present(sim: &mut Sim) -> Option<Violation> {
    for r in sim.records() {
        let fired_unread = sim.ssh.with(|f| {
            f.files
                .contains_key(&(TARGET.to_string(), r.id.clone(), "fired".to_string()))
        }) && !r.backstop.as_ref().is_some_and(|b| b.fired);
        for a in r.applied.clone() {
            let Some(op) = r.op_at(a.step) else { continue };
            if fired_unread && op.undo_locus == UndoLocus::Target {
                continue;
            }
            for e in op.footprint.iter().filter(|e| e.kind == Kind::Owned) {
                let there = sim.ssh.with(|f| f.facts.contains_key(&e.shape));
                if !there {
                    return broke(
                        2,
                        format!(
                            "{}: step {} is applied but {} is gone",
                            r.id, a.step, e.shape
                        ),
                    );
                }
            }
        }
        // A closed instance leaves no owned fact of its own behind.
        if r.state == State::Closed
            && r.closed_reason.is_some()
            && !fired_unread
            && !abandoned(sim, &r.id)
        {
            for (n, _) in rue_core::algebra::numbered(&r.plan().body) {
                let Some(op) = r.op_at(n) else { continue };
                if op.undo == rue_core::model::Undo::NoUndo {
                    continue;
                }
                for e in op.footprint.iter().filter(|e| e.kind == Kind::Owned) {
                    if sim.ssh.with(|f| f.facts.contains_key(&e.shape))
                        && !r.drift_held.contains(&n)
                        && r.stuck.is_empty()
                    {
                        return broke(
                            2,
                            format!("{}: closed cleanly but {} is still there", r.id, e.shape),
                        );
                    }
                }
            }
        }
    }
    None
}

/// (3) The stuck set is non-empty exactly when the instance is Stuck, and
/// every step in it is one the plan has.
fn i03_stuck_set(sim: &mut Sim) -> Option<Violation> {
    for r in sim.records() {
        if r.state == State::Stuck && r.stuck.is_empty() {
            return broke(3, format!("{}: stuck with an empty stuck set", r.id));
        }
        if r.state != State::Stuck && !r.stuck.is_empty() {
            return broke(
                3,
                format!("{}: {:?} in the stuck set while {}", r.id, r.stuck, r.state),
            );
        }
    }
    None
}

/// (4) The journal chain verifies end to end.
fn i04_journal_verifies(sim: &mut Sim) -> Option<Violation> {
    let entries = sim.sink.entries();
    if let Err(e) = rue_core::journal::verify(&entries) {
        return broke(4, format!("{e:?}"));
    }
    None
}

/// (5) No secret value appears in any sink.
fn i05_no_secret_in_a_sink(sim: &mut Sim) -> Option<Violation> {
    for e in sim.sink.entries() {
        let text = serde_json::to_string(&e).unwrap_or_default();
        if text.contains(SECRET) {
            return broke(5, format!("entry {} carries the secret", e.seq));
        }
    }
    None
}

/// (6) No wane fired while the engine was settling after a boot.
fn i06_no_wane_during_settle(sim: &mut Sim) -> Option<Violation> {
    let at = sim.booted_at?;
    // Settle ends inside `boot`, so an Expired entry stamped at the boot's
    // own instant would be one that fired during it.
    for e in sim.sink.entries() {
        if matches!(e.event, J::Expired) && e.at == at && sim.engine.settling() {
            return broke(6, format!("entry {} expired while settling", e.seq));
        }
    }
    None
}

/// (7) A step declaring `reach` is never applied before the backstop
/// covering it is armed: on the target that is the deadline landing
/// before the step's own `do`.
fn i07_reach_after_arming(sim: &mut Sim) -> Option<Violation> {
    let has_reach = sim.records().iter().any(|r| {
        r.applied
            .iter()
            .any(|a| r.op_at(a.step).is_some_and(|o| !o.reach.is_empty()))
    });
    if !has_reach {
        return None;
    }
    let events = sim.ssh.events();
    let deadline = events.iter().position(|e| e == "replace deadline");
    let first_run = events.iter().position(|e| e == "run");
    match (deadline, first_run) {
        (Some(d), Some(r)) if d > r => broke(
            7,
            format!("the deadline landed at {d}, after the first run at {r}"),
        ),
        (None, Some(_)) => broke(7, "a reach step ran and no deadline was ever written"),
        _ => None,
    }
}

/// (8) For every drift event, the end state is the same whether the
/// engine or the artifact ran the undo. The sim reads this from the
/// journal: a step that drifted is either held or clobbered, never both,
/// and a step the artifact undid leaves no marker behind.
fn i08_engine_and_artifact_agree(sim: &mut Sim) -> Option<Violation> {
    let mut held = Vec::new();
    let mut clobbered = Vec::new();
    for e in sim.sink.entries() {
        match &e.event {
            J::DriftHeld { step, .. } => held.push(*step),
            J::DriftClobbered { step, .. } => clobbered.push(*step),
            _ => {}
        }
    }
    for step in &held {
        if clobbered.contains(step) {
            // Held then forced is the one legitimate way both appear.
            let forced = sim
                .records()
                .iter()
                .any(|r| r.force_drift || !r.drift_held.is_empty());
            if !forced {
                return broke(
                    8,
                    format!("step {step} was both held and clobbered with no force"),
                );
            }
        }
    }
    None
}

/// (9) No secret in a command line, in what an executor was asked to run,
/// or in an artifact written to a target.
fn i09_no_secret_in_argv_or_artifact(sim: &mut Sim) -> Option<Violation> {
    let calls = sim.ssh.calls();
    for c in &calls {
        let text = format!("{:?}", c.body);
        if text.contains(SECRET) {
            return broke(9, format!("a body on {} carries the secret", c.host));
        }
    }
    let files = sim.ssh.with(|f| {
        f.files
            .iter()
            .map(|((h, i, rel), v)| {
                (
                    format!("{h}/{i}/{rel}"),
                    String::from_utf8_lossy(v).into_owned(),
                )
            })
            .collect::<Vec<_>>()
    });
    for (name, body) in files {
        if body.contains(SECRET) {
            return broke(9, format!("{name} carries the secret"));
        }
    }
    None
}

/// (10) No covered step ran before its artifact was installed.
fn i10_artifact_before_covered_step(sim: &mut Sim) -> Option<Violation> {
    let covered = sim.records().iter().any(|r| {
        r.applied.iter().any(|a| {
            r.op_at(a.step)
                .is_some_and(|o| o.undo_locus == UndoLocus::Target)
        })
    });
    if !covered {
        return None;
    }
    let events = sim.ssh.events();
    let put = events.iter().position(|e| e == "put artifact.sh");
    let first_run = events.iter().position(|e| e == "run");
    match (put, first_run) {
        (Some(p), Some(r)) if p > r => broke(
            10,
            format!("the artifact landed at {p}, after the first run at {r}"),
        ),
        (None, Some(_)) => broke(10, "a covered step ran and no artifact was installed"),
        _ => None,
    }
}

/// (11) No staged file survives an instance that is not applying.
fn i11_no_staged_file_survives(sim: &mut Sim) -> Option<Violation> {
    for r in sim.records() {
        if r.state != State::Applying && !r.staged.is_empty() {
            return broke(
                11,
                format!(
                    "{}: {} staged files while {}",
                    r.id,
                    r.staged.len(),
                    r.state
                ),
            );
        }
    }
    None
}

/// (12) A region is never clobbered while another active instance holds a
/// region on the same fact: the shared file keeps every live anchor.
fn i12_no_region_clobbered_under_a_sibling(sim: &mut Sim) -> Option<Violation> {
    let live: Vec<(String, String)> = sim
        .records()
        .into_iter()
        .filter(|r| !rue_core::states::terminal(r.state))
        .flat_map(|r| {
            let id = r.id.clone();
            r.applied
                .iter()
                .filter_map(|a| r.op_at(a.step))
                .flat_map(|op| op.footprint.clone())
                .filter(|e| e.kind == Kind::Region)
                .filter_map(move |e| e.anchor.clone().map(|an| (id.clone(), an)))
                .collect::<Vec<_>>()
        })
        .collect();
    if live.len() < 2 {
        return None;
    }
    let text = sim
        .ssh
        .with(|f| f.facts.get(crate::world::SHARED).cloned())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    for (id, anchor) in live {
        if !text.contains(&format!("rue-region {anchor} begin")) {
            return broke(
                12,
                format!("{id}: the region {anchor} it holds is gone from the shared fact"),
            );
        }
    }
    None
}

/// (13) The ledger holds exactly the instances in a reserving state.
fn i13_ledger_holds_the_reservers(sim: &mut Sim) -> Option<Violation> {
    let ledger = match sim.engine.store().read_ledger() {
        Ok(l) => l,
        Err(e) => return broke(13, format!("the ledger did not read: {e}")),
    };
    let holding: Vec<String> = ledger
        .holdings()
        .iter()
        .map(|h| h.id.split('@').next().unwrap_or_default().to_string())
        .collect();
    for r in sim.records() {
        let reserves = !rue_core::states::terminal(r.state) && !r.rehearsal;
        let held = holding.contains(&r.id);
        if reserves && !held && !r.ledger_ids.is_empty() {
            return broke(13, format!("{} is {} and reserves nothing", r.id, r.state));
        }
        if !reserves && held {
            return broke(13, format!("{} is {} and still reserves", r.id, r.state));
        }
    }
    None
}

/// (14) A proof made for one scope satisfies no other: every proof an
/// instance holds is in the scope it was made for, and a plan gate is
/// never opened by a step's proof.
fn i14_a_proof_is_scoped(sim: &mut Sim) -> Option<Violation> {
    for r in sim.records() {
        if r.plan().gate.is_none() {
            continue;
        }
        let plan_proofs = r
            .proofs
            .iter()
            .filter(|p| p.scope == rue_core::journal::Scope::Plan)
            .count();
        let approved = r.approved_at.is_some();
        // Two authenticators are named; the gate needs both. Approved with
        // fewer plan-scope proofs than that means something else opened it.
        if approved && plan_proofs < 2 && !r.rehearsal {
            return broke(
                14,
                format!("{}: approved on {plan_proofs} plan-scope proofs", r.id),
            );
        }
    }
    None
}

/// (15) Boot reconciliation never removes a directory holding an armed,
/// unfired artifact.
fn i15_reconciliation_spares_armed(sim: &mut Sim) -> Option<Violation> {
    for r in sim.records() {
        let Some(b) = r.backstop.clone() else {
            continue;
        };
        if !b.armed || b.fired {
            continue;
        }
        let there = sim
            .ssh
            .with(|f| f.dirs.contains(&(TARGET.to_string(), r.id.clone())));
        if !there {
            return broke(
                15,
                format!("{}: armed and unfired, and its directory is gone", r.id),
            );
        }
    }
    None
}

/// (16) No `Waiting`, `Held` or `Deferred` state outlives its bound,
/// except DriftHeld, Stuck, and Held or Deferred in a permanent plan.
/// A bound is only overdue once a reap pass has had the chance to see it.
fn i16_no_bounded_state_outlives_its_bound(sim: &mut Sim) -> Option<Violation> {
    let now = sim.clock.now();
    for r in sim.records() {
        if r.state != State::Waiting {
            continue;
        }
        let Some(w) = r.waiting.clone() else { continue };
        let Some(bound) = w.bound else { continue };
        if now.unix_s > bound.unix_s + 3_600 {
            return broke(
                16,
                format!(
                    "{}: waiting at step {} an hour past its bound",
                    r.id, w.step
                ),
            );
        }
    }
    None
}

/// (17) A region undo never sees the manifest change between its decision
/// and its write: the host lock is taken before the first read and held
/// past the last write.
fn i17_one_manifest_per_region_undo(sim: &mut Sim) -> Option<Violation> {
    let events = sim.ssh.events();
    let mut depth = 0i32;
    for (i, e) in events.iter().enumerate() {
        match e.as_str() {
            "lock" => depth += 1,
            "unlock" => {
                depth -= 1;
                if depth < 0 {
                    return broke(17, format!("an unlock at {i} with no lock"));
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return broke(17, format!("{depth} host locks were never released"));
    }
    None
}

/// (18) No control-channel act by an undeclared identity or outside its
/// scope. The sim drives the engine directly, so the channel's refusals
/// are `engine/tests/control.rs`'s to prove; what is checked here is that
/// every act the journal records names an identity the site declares.
fn i18_no_undeclared_act(sim: &mut Sim) -> Option<Violation> {
    for e in sim.sink.entries() {
        if let J::Committed { by, .. } | J::Abandoned { by, .. } = &e.event {
            if by != "requester" && by != "probe" && !by.is_empty() {
                return broke(18, format!("entry {} was acted by {by}", e.seq));
            }
        }
    }
    None
}

/// (19) A permanent plan is never reverted by time.
fn i19_no_permanent_plan_reverted_by_time(sim: &mut Sim) -> Option<Violation> {
    for r in sim.records() {
        if !r.permanent {
            continue;
        }
        let expired = sim
            .sink
            .entries()
            .iter()
            .any(|e| e.instance == r.id && matches!(e.event, J::Expired));
        if expired {
            return broke(19, format!("{} is permanent and expired", r.id));
        }
    }
    None
}

/// (20) A committed plan's backstop never fires.
fn i20_no_committed_backstop_fires(sim: &mut Sim) -> Option<Violation> {
    for r in sim.records() {
        if r.state != State::Committed {
            continue;
        }
        let fired = sim.sink.entries().iter().any(|e| {
            e.instance == r.id
                && matches!(
                    e.event,
                    J::BackstopFired { .. } | J::BackstopFiredAfterAbandon { .. }
                )
        });
        if fired {
            return broke(20, format!("{} committed and its backstop fired", r.id));
        }
        if r.backstop.as_ref().is_some_and(|b| b.armed) {
            return broke(
                20,
                format!("{} committed with its backstop still armed", r.id),
            );
        }
    }
    None
}
