//! Backstops over the fakes (5.6, 7.7): the artifact installed before the
//! first covered step and armed per `arm_before` (before the step where
//! `reach` demands it, after the last covered step where the plan arms
//! late); a host with no scheduler binding R0401; a target whose clock is
//! beyond tolerance R0403; an instance directory with the wrong modes
//! R0406; renewal rearming before the new expiry is the instance's and a
//! failing rearm refusing it R0404; `confirm()` disarming the
//! `unless_confirmed` trigger and `commit()` disarming everything before
//! the directory goes; `abandon` saying what it left armed; the heartbeat
//! written at its interval; the `fired` marker read on the next contact
//! (R0402); boot reconciliation leaving an armed orphan and reclaiming a
//! fired one; and `rue reclaim` refused while armed (R0405) until it is
//! forced with a reason.

mod common;

use std::collections::BTreeMap;

use common::world::{self, World, OWNER, T0};
use rue_core::model::{Backstop, Duration, Instant, Item, Plan, Trigger};
use rue_core::states::State;
use rue_engine::lifecycle::ApplyOptions;

fn opts() -> ApplyOptions {
    ApplyOptions {
        by: "ops".into(),
        ..ApplyOptions::default()
    }
}

/// A temporary plan whose backstop expires with its wane, covering every
/// step, armed before step `arm_before`.
fn temp_with_backstop(id: &str, steps: usize, arm_before: u32) -> Plan {
    let body: Vec<Item> = (1..=steps)
        .map(|i| world::step(world::covered(&format!("s{i}"))))
        .collect();
    let mut p = world::temp_plan(id, body);
    p.backstop = Some(Backstop {
        triggers: vec![Trigger::After(Duration::new(3600))],
        arm_before,
    });
    p
}

fn ops(w: &World) -> Vec<String> {
    w.sched.ops()
}

#[test]
fn the_artifact_is_installed_before_the_first_covered_step_and_armed_before_the_step_that_needs_it()
{
    let mut w = World::new("bs-arm");
    let plan = temp_with_backstop("p", 2, 1);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let o = ops(&w);
    assert!(
        o.iter().any(|x| x.starts_with("install h")),
        "the entry is installed: {o:?}"
    );
    assert!(o.iter().any(|x| x.starts_with("arm h")), "armed: {o:?}");
    // Armed before the covered step ran, which the host sees as the
    // deadline landing before the first `do`: no step is committed before
    // the backstop covering it is armed (5.6).
    let order = w.ssh.events();
    let deadline = order
        .iter()
        .position(|e| e == "replace deadline")
        .expect("the arm wrote a deadline");
    let first_run = order.iter().position(|e| e == "run").expect("the step ran");
    assert!(
        deadline < first_run,
        "the deadline is on the target before the step it covers: {order:?}"
    );
    // The artifact and the deadline are in the instance directory.
    let files = w.ssh.with(|f| {
        f.files
            .keys()
            .filter(|(_, i, _)| i == &out.id)
            .map(|(_, _, r)| r.clone())
            .collect::<Vec<_>>()
    });
    assert!(
        files.iter().any(|r| r == "artifact.sh") && files.iter().any(|r| r == "deadline"),
        "{files:?}"
    );
    // The deadline the artifact reads is the instance's wane.
    let deadline = w
        .ssh
        .with(|f| {
            f.files
                .get(&(OWNER.into(), out.id.clone(), "deadline".into()))
                .cloned()
        })
        .map(|b| String::from_utf8_lossy(&b).trim().to_string());
    assert_eq!(deadline.as_deref(), Some((T0 + 3600).to_string().as_str()));
}

#[test]
fn a_plan_that_arms_late_arms_after_its_last_covered_step() {
    let mut w = World::new("bs-late");
    // arm_before past the last step: the late-arming window of 5.6.
    let plan = temp_with_backstop("p", 2, 3);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Applied, "{}", out.line);
    let o = ops(&w);
    assert!(o.iter().any(|x| x.starts_with("arm h")), "{o:?}");
    // Both covered steps ran before the arm: the engine-only window the
    // verdict states for a plan with no `reach`.
    let order = w.ssh.events();
    let deadline = order
        .iter()
        .position(|e| e == "replace deadline")
        .expect("the arm wrote a deadline");
    let runs: Vec<usize> = order
        .iter()
        .enumerate()
        .filter(|(_, e)| e.as_str() == "run")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(runs.len(), 2, "{order:?}");
    assert!(runs[1] < deadline, "armed last: {order:?}");
}

#[test]
fn a_host_whose_scheduler_the_site_never_bound_is_r0401_and_the_plan_reverts() {
    let mut w = World::new("bs-r0401");
    // The world binds one scheduler named cron; a host naming another has
    // none.
    w.engine.set_hosts(vec![
        {
            let mut h = world::host(OWNER, &["ssh"]);
            h.scheduler = Some("task_scheduler".into());
            h
        },
        world::host(world::FAR, &["carrier-pigeon"]),
    ]);
    let plan = temp_with_backstop("p", 1, 1);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert!(
        w.events().iter().any(|e| e.contains("R0401")),
        "{:?}",
        w.events()
    );
}

#[test]
fn a_target_clock_beyond_tolerance_refuses_to_arm_and_a_wrong_mode_directory_refuses_too() {
    let mut w = World::new("bs-r0403");
    // The target's clock is ten minutes off; the tolerance is two.
    w.ssh.with(|f| f.clock = Some(Instant::new(T0 + 600)));
    let plan = temp_with_backstop("p", 1, 1);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    let events = w.events();
    assert!(events.iter().any(|e| e.contains("R0403")), "{events:?}");
    assert!(
        !ops(&w).iter().any(|x| x.starts_with("arm ")),
        "nothing was armed: {:?}",
        ops(&w)
    );

    // A directory whose modes are wrong refuses arming (R0406).
    let mut w = World::new("bs-r0406");
    w.ssh.with(|f| {
        f.clock = Some(Instant::new(T0));
        f.dir_modes_ok = false;
    });
    let plan = temp_with_backstop("p", 1, 1);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Closed, "{}", out.line);
    assert!(
        w.events().iter().any(|e| e.contains("R0406")),
        "{:?}",
        w.events()
    );
}

#[test]
fn renewal_rearms_before_the_new_expiry_is_the_instance_s_and_a_failing_rearm_refuses_it() {
    let mut w = World::new("bs-renew");
    let plan = temp_with_backstop("p", 1, 1);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    // Inside renew_within of the wane deadline.
    w.advance(3_100);
    w.engine.renew(&id, Duration::new(3600)).unwrap();
    let o = ops(&w);
    assert!(o.iter().any(|x| x.starts_with("rearm h")), "{o:?}");
    let deadline = w
        .ssh
        .with(|f| {
            f.files
                .get(&(OWNER.into(), id.clone(), "deadline".into()))
                .cloned()
        })
        .map(|b| String::from_utf8_lossy(&b).trim().to_string());
    assert_eq!(
        deadline.as_deref(),
        Some((T0 + 3_100 + 3600).to_string().as_str()),
        "the artifact's deadline moved with the instance's"
    );
    let before = w.engine.status(&id).unwrap().unwrap().deadline;

    // A rearm that fails refuses the renewal, and the deadline stands.
    w.sched.fail("rearm");
    w.advance(3_100);
    let err = w.engine.renew(&id, Duration::new(3600)).unwrap_err();
    assert!(err.to_string().contains("R0404"), "{err}");
    assert_eq!(
        w.engine.status(&id).unwrap().unwrap().deadline,
        before,
        "the instance's expiry never moved"
    );
}

#[test]
fn commit_disarms_everything_before_the_instance_directory_goes() {
    let mut w = World::new("bs-commit");
    let mut body: Vec<Item> = vec![world::step(world::covered("s1"))];
    body.push(Item::Commit);
    let mut plan = Plan::new("p", OWNER, body);
    plan.backstop = Some(Backstop {
        triggers: vec![Trigger::UnlessConfirmed(Duration::new(600))],
        arm_before: 1,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Committed, "{}", out.line);
    let o = ops(&w);
    let disarm = o.iter().position(|x| x.starts_with("disarm h"));
    assert!(disarm.is_some(), "{o:?}");
    assert!(
        w.sched.entries().is_empty(),
        "the entry is gone: {:?}",
        w.sched.entries()
    );
    // The directory went after the disarm, not before.
    let dirs = w.ssh.with(|f| f.dirs.clone());
    assert!(dirs.is_empty(), "{dirs:?}");
    let events = w.ssh.events();
    let last_disarm = events
        .iter()
        .rposition(|e| e.starts_with("remove artifact.sh"));
    assert!(last_disarm.is_some(), "{events:?}");
}

#[test]
fn confirm_takes_the_deadline_away_and_commit_takes_the_entry_after_it() {
    let mut w = World::new("bs-confirm");
    // A permanent plan: step, confirm(), step, commit(). The confirm
    // disarms the `unless_confirmed` trigger; the heartbeat still has
    // something to say, so the entry stays until the commit.
    let body: Vec<Item> = vec![
        world::step(world::covered("s1")),
        Item::Confirm,
        world::step(world::op("s2")),
        Item::Commit,
    ];
    let mut plan = Plan::new("p", OWNER, body);
    plan.backstop = Some(Backstop {
        triggers: vec![
            Trigger::UnlessConfirmed(Duration::new(600)),
            Trigger::UnlessHeartbeat {
                deadline: Duration::new(60),
                interval: Some(Duration::new(20)),
            },
        ],
        arm_before: 1,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    assert_eq!(out.state, State::Committed, "{}", out.line);
    // One disarm, at the commit; the deadline went earlier, at the confirm.
    let o = ops(&w);
    assert_eq!(
        o.iter().filter(|x| x.starts_with("disarm h")).count(),
        1,
        "{o:?}"
    );
    let events = w.ssh.events();
    let removed_deadline = events
        .iter()
        .position(|e| e == "remove deadline")
        .expect("the confirm removed the deadline the artifact reads");
    let removed_artifact = events
        .iter()
        .rposition(|e| e == "remove artifact.sh")
        .expect("the commit removed the artifact");
    assert!(
        removed_deadline < removed_artifact,
        "confirm first, commit after: {events:?}"
    );
    // The second step ran between them: the entry served the heartbeat
    // while the plan finished.
    let ran = events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.as_str() == "run")
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    assert!(
        ran.iter()
            .any(|i| *i > removed_deadline && *i < removed_artifact),
        "{events:?}"
    );
}

#[test]
fn the_heartbeat_is_written_at_its_interval_and_a_temporary_plan_may_have_one() {
    let mut w = World::new("bs-heartbeat");
    let mut plan = temp_with_backstop("p", 1, 1);
    plan.backstop = Some(Backstop {
        // A temporary plan's after: is its wane, and it MAY add a
        // heartbeat (5.6).
        triggers: vec![
            Trigger::After(Duration::new(3600)),
            Trigger::UnlessHeartbeat {
                deadline: Duration::new(60),
                interval: Some(Duration::new(20)),
            },
        ],
        arm_before: 1,
    });
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    let beat = |w: &World| {
        w.ssh
            .with(|f| {
                f.files
                    .get(&(OWNER.into(), id.clone(), "heartbeat".into()))
                    .cloned()
            })
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
    };
    assert_eq!(beat(&w).as_deref(), Some(T0.to_string().as_str()));
    // Before the interval: nothing written.
    w.advance(10);
    assert!(w.engine.heartbeat().unwrap().is_empty());
    assert_eq!(beat(&w).as_deref(), Some(T0.to_string().as_str()));
    // At the interval: a fresh beat.
    w.advance(10);
    assert_eq!(w.engine.heartbeat().unwrap(), vec![id.clone()]);
    assert_eq!(beat(&w).as_deref(), Some((T0 + 20).to_string().as_str()));
}

#[test]
fn a_fired_artifact_is_read_on_the_next_contact_and_its_steps_are_no_longer_applied() {
    let mut w = World::new("bs-fired");
    let plan = temp_with_backstop("p", 2, 1);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    assert_eq!(w.engine.status(&id).unwrap().unwrap().applied.len(), 2);
    // The target fires: the marker of step 2 is gone and `fired` is there.
    w.ssh.with(|f| {
        f.files
            .insert((OWNER.into(), id.clone(), "fired".into()), b"".to_vec());
        f.files
            .remove(&(OWNER.into(), id.clone(), "markers/2".into()));
    });
    let report = w.engine.reap().unwrap();
    assert!(
        report.actions.iter().any(|a| a.contains("backstop fired")),
        "{:?}",
        report.actions
    );
    let events = w.events();
    assert!(
        events
            .iter()
            .any(|e| e.contains("BackstopFired { step: 2 }")),
        "{events:?}"
    );
    let rec = w.engine.status(&id).unwrap().unwrap();
    assert_eq!(
        rec.applied.iter().map(|a| a.step).collect::<Vec<_>>(),
        vec![1],
        "the target undid step 2"
    );
}

#[test]
fn boot_leaves_an_armed_orphan_where_it_is_and_reclaims_a_fired_one() {
    let mut w = World::new("bs-reconcile");
    // Two directories the store knows nothing about: one armed, one fired.
    w.ssh.with(|f| {
        f.dirs.insert((OWNER.into(), "i-armed".into()));
        f.files.insert(
            (OWNER.into(), "i-armed".into(), "artifact.sh".into()),
            b"#!/bin/sh\n".to_vec(),
        );
        f.dirs.insert((OWNER.into(), "i-fired".into()));
        f.files.insert(
            (OWNER.into(), "i-fired".into(), "artifact.sh".into()),
            b"#!/bin/sh\n".to_vec(),
        );
        f.files.insert(
            (OWNER.into(), "i-fired".into(), "fired".into()),
            b"".to_vec(),
        );
    });
    let report = w.engine.boot().unwrap();
    assert_eq!(
        report.orphaned,
        vec![(OWNER.to_string(), "i-armed".to_string())]
    );
    assert_eq!(
        report.reclaimed,
        vec![(OWNER.to_string(), "i-fired".to_string())]
    );
    let dirs = w.ssh.with(|f| f.dirs.clone());
    assert!(dirs.contains(&(OWNER.into(), "i-armed".into())));
    assert!(!dirs.contains(&(OWNER.into(), "i-fired".into())));
    let events = w.events();
    assert!(
        events
            .iter()
            .any(|e| e.contains("InstanceDirOrphaned") && e.contains("armed: true")),
        "{events:?}"
    );
    assert!(events.iter().any(|e| e.contains("Reclaimed")), "{events:?}");
}

#[test]
fn reclaim_is_refused_while_the_artifact_is_armed_and_its_entry_present_until_it_is_forced() {
    let mut w = World::new("bs-reclaim");
    w.ssh.with(|f| {
        f.dirs.insert((OWNER.into(), "i-armed".into()));
        f.files.insert(
            (OWNER.into(), "i-armed".into(), "artifact.sh".into()),
            b"#!/bin/sh\n".to_vec(),
        );
    });
    // The scheduler holds an entry for it.
    w.sched
        .with(|f| f.entries.push((OWNER.to_string(), "i-armed".to_string())));
    let err = w
        .engine
        .reclaim(OWNER, "i-armed", false, "")
        .unwrap_err()
        .to_string();
    assert!(err.contains("R0405") && err.contains("present"), "{err}");
    // Forced without a reason is still refused.
    let err = w
        .engine
        .reclaim(OWNER, "i-armed", true, "  ")
        .unwrap_err()
        .to_string();
    assert!(err.contains("needs a --reason"), "{err}");
    // Forced with one is accepted and journaled.
    w.engine
        .reclaim(OWNER, "i-armed", true, "the artifact was read")
        .unwrap();
    assert!(!w
        .ssh
        .with(|f| f.dirs.contains(&(OWNER.into(), "i-armed".into()))));
    assert!(
        w.events()
            .iter()
            .any(|e| e.contains("Reclaimed") && e.contains("forced: true")),
        "{:?}",
        w.events()
    );
}

#[test]
fn a_backstop_left_armed_by_an_abandon_is_journaled_when_it_fires() {
    let mut w = World::new("bs-after-abandon");
    let plan = temp_with_backstop("p", 1, 1);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    w.ssh.with(|f| {
        f.script
            .push_back(rue_engine::executor::Scripted::Fail("no".into()))
    });
    w.engine.recant(&id, &[]).unwrap();
    w.sched.fail("disarm");
    w.engine.abandon(&id, "ops", "the host is gone").unwrap();
    assert_eq!(w.engine.status(&id).unwrap().unwrap().state, State::Closed);
    // Later the artifact fires on its own.
    w.ssh.with(|f| {
        f.files
            .insert((OWNER.into(), id.clone(), "fired".into()), b"".to_vec());
    });
    let report = w.engine.reap().unwrap();
    assert!(
        report
            .actions
            .iter()
            .any(|a| a.contains("fired after abandon")),
        "{:?}",
        report.actions
    );
    assert!(
        w.events()
            .iter()
            .any(|e| e.contains("BackstopFiredAfterAbandon")),
        "{:?}",
        w.events()
    );
    // Read once: a second pass says nothing more.
    let again = w.engine.reap().unwrap();
    assert!(!again
        .actions
        .iter()
        .any(|a| a.contains("fired after abandon")));
}

#[test]
fn doctor_lists_the_armed_orphans_reconciliation_left_in_place() {
    let mut w = World::new("bs-doctor");
    w.ssh.with(|f| {
        f.dirs.insert((OWNER.into(), "i-armed".into()));
        f.files.insert(
            (OWNER.into(), "i-armed".into(), "artifact.sh".into()),
            b"#!/bin/sh\n".to_vec(),
        );
    });
    let r = w.engine.doctor().unwrap();
    assert_eq!(
        r.orphans,
        vec![(OWNER.to_string(), "i-armed".to_string())],
        "{r:?}"
    );
    // The host's scheduler is bound by the site, and doctor says so.
    assert!(r.hosts.iter().any(|h| h.name == OWNER && h.scheduler_bound));
}

#[test]
fn abandon_disarms_where_it_can_and_says_what_it_left_armed() {
    let mut w = World::new("bs-abandon");
    let plan = temp_with_backstop("p", 1, 1);
    let out = w
        .engine
        .apply(world::ir(plan), BTreeMap::new(), opts())
        .unwrap();
    let id = out.id.clone();
    // The instance must be abandonable: a stuck undo puts it there.
    w.ssh.with(|f| {
        f.script
            .push_back(rue_engine::executor::Scripted::Fail("no".into()))
    });
    w.engine
        .recant(&id, &[])
        .expect("recant reverts and gets stuck");
    assert_eq!(
        w.engine.status(&id).unwrap().unwrap().state,
        State::Stuck,
        "the undo failed"
    );
    // The host refuses the disarm: the artifact stays armed and is named.
    w.sched.fail("disarm");
    w.engine.abandon(&id, "ops", "the host is gone").unwrap();
    let events = w.events();
    assert!(
        events.iter().any(|e| e.contains("Abandoned")
            && e.contains("artifacts_left_armed")
            && !e.contains("artifacts_left_armed: []")),
        "{events:?}"
    );
}
