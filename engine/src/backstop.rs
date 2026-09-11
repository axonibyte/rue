//! Backstops at runtime, docs/ROADMAP.md 5.6 and 7.7: rendering the
//! `:target` artifact into the instance directory, registering it with the
//! host's scheduler, arming it by writing a deadline, keeping a heartbeat,
//! rearming on renewal, disarming on `confirm()` and `commit()`, reading
//! the marker a fired artifact leaves, and reconciling the directories a
//! restarted engine finds on its hosts.
//!
//! The positions this module takes where the roadmap is silent are stated
//! in docs/DESIGN.md: one artifact per instance, so every covered step
//! must be on one host; a scheduler that cannot say whether its entry is
//! there is never read as absence; a heartbeat is written at arm and then
//! at its interval, so an engine that stops is itself the thing the
//! trigger notices.

use std::collections::BTreeMap;

use rue_core::backstop::coverage;
use rue_core::journal::Event as J;
use rue_core::model::{Duration, Instant, Trigger};
use rue_render::{render, Bindings, Instance as RInstance};
use serde::{Deserialize, Serialize};

use crate::gates::{hex, nonce};
use crate::host::Host;
use crate::lifecycle::{applied_steps, Engine, EngineError, InstanceRecord};
use crate::scheduler::{Job, Presence, Scheduler};

/// The site's default when it declares none (7.7).
pub const DEFAULT_SKEW_TOLERANCE: Duration = Duration { seconds: 120 };

/// What the engine has put on a host for this instance's backstop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackstopState {
    pub host: String,
    /// The artifact's file name in the instance directory.
    pub artifact: String,
    /// The scheduler binding that holds the entry.
    pub scheduler: String,
    pub installed: bool,
    pub armed: bool,
    /// The deadline written to the instance directory, when a trigger
    /// wants one.
    pub deadline: Option<Instant>,
    /// `unless_heartbeat`: how stale a heartbeat may be, and how often the
    /// engine writes one.
    pub heartbeat_s: Option<u64>,
    pub interval_s: Option<u64>,
    pub last_heartbeat: Option<Instant>,
    /// `confirm()` has disarmed the `unless_confirmed` trigger.
    pub confirmed: bool,
    /// The artifact left its `fired` marker and the engine has read it.
    pub fired: bool,
}

/// The deadline and heartbeat an arm writes, from the plan's triggers
/// (5.6). A temporary plan's `after:` is its wane by E0503, so the
/// instance's own wane deadline is what the artifact compares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schedule {
    pub deadline: Option<Instant>,
    pub heartbeat_s: Option<u64>,
    pub interval_s: Option<u64>,
}

pub fn schedule(triggers: &[Trigger], now: Instant, wane: Option<Instant>) -> Schedule {
    let mut s = Schedule::default();
    for t in triggers {
        match t {
            // The wane deadline is the instance's, anchored at approval or
            // the last renewal; the trigger's own duration equals it.
            Trigger::After(d) => s.deadline = Some(wane.unwrap_or_else(|| now.plus(*d))),
            Trigger::UnlessConfirmed(d) => s.deadline = Some(now.plus(*d)),
            Trigger::UnlessHeartbeat { deadline, interval } => {
                s.heartbeat_s = Some(deadline.seconds);
                // A third of the deadline is the ceiling E0405 already
                // enforces; with no interval declared it is the interval.
                s.interval_s = Some(interval.map(|i| i.seconds).unwrap_or(deadline.seconds / 3));
            }
        }
    }
    s
}

/// What reconciliation found: instance directories left in place because
/// they hold an armed artifact, directories reclaimed, and directories
/// left alone because another controller stamped them.
pub type Reconciled = (
    Vec<(String, String)>,
    Vec<(String, String)>,
    Vec<(String, String)>,
);

/// What `rue doctor` finds among the instance directories: armed orphans,
/// and directories another controller stamped.
pub type Unclaimed = (Vec<(String, String)>, Vec<(String, String)>);

/// The distance between two clocks, in seconds, whichever is ahead.
pub fn skew(a: Instant, b: Instant) -> u64 {
    a.unix_s.abs_diff(b.unix_s)
}

/// What a disarm covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disarm {
    /// `confirm()`: the `unless_confirmed` trigger only.
    Confirmed,
    /// `commit()`, `abandon`, a clean revert: the entry and the artifact.
    All,
}

fn epoch_text(i: Instant) -> Vec<u8> {
    format!("{}\n", i.unix_s).into_bytes()
}

impl Engine {
    /// The scheduler a host declares, if the site bound one by that name.
    pub(crate) fn scheduler_named(&mut self, name: &str) -> Option<&mut Box<dyn Scheduler>> {
        self.schedulers.iter_mut().find(|s| s.name() == name)
    }

    /// The host a `:target` backstop lives on: the host of the covered
    /// steps. One artifact undoes them all, so they must share a host.
    pub(crate) fn backstop_host(&self, rec: &InstanceRecord) -> Result<Option<Host>, String> {
        let Some(cov) = coverage(rec.plan()) else {
            return Ok(None);
        };
        let mut found: Option<Host> = None;
        for n in &cov.covered {
            let Some(op) = rec.op_at(*n) else { continue };
            let h = self.step_host(rec, op)?;
            match &found {
                None => found = Some(h),
                Some(f) if f.name() != h.name() => {
                    return Err(format!(
                        "the backstop covers steps on {} and on {}: one artifact undoes one host's steps",
                        f.name(),
                        h.name()
                    ))
                }
                Some(_) => {}
            }
        }
        Ok(found)
    }

    /// Render the artifact for this instance on its host.
    fn render_artifact(
        &self,
        rec: &InstanceRecord,
        host: &Host,
    ) -> Result<(&'static str, String), String> {
        let inst = RInstance {
            id: rec.id.clone(),
            rue_root: host.rue_root.clone(),
        };
        let mut b = Bindings {
            params: rec.params.clone(),
            host_fields: BTreeMap::new(),
        };
        for (k, v) in &host.facts {
            b.host_fields.insert(k.clone(), v.clone());
        }
        b.host_fields
            .insert("address".to_string(), host.address.clone());
        match render(&rec.ir.site, rec.plan(), host.name(), &inst, &b) {
            Ok(a) => Ok((a.file_name, a.text)),
            Err(e) => Err(format!("the backstop artifact for {}: {e}", host.name())),
        }
    }

    fn job_of(&self, rec: &InstanceRecord, host: &Host, st: &BackstopState) -> Job {
        let shell = rue_core::artifact::shell_of(&host.record.os);
        let root = RInstance {
            id: rec.id.clone(),
            rue_root: host.rue_root.clone(),
        }
        .root(shell);
        let sep = if matches!(shell, rue_core::artifact::Shell::Powershell) {
            "\\"
        } else {
            "/"
        };
        Job {
            instance: rec.id.clone(),
            artifact: format!("{root}{sep}instances{sep}{}{sep}{}", rec.id, st.artifact),
            language: rue_core::artifact::language_of(&host.record),
            os: host.record.os.clone(),
        }
    }

    /// Install the artifact and its scheduler entry, before the first
    /// covered step (7.7). `Ok(Err(reason))` is a refusal of the step that
    /// wanted it.
    pub(crate) fn backstop_install(
        &mut self,
        rec: &mut InstanceRecord,
    ) -> Result<Result<(), String>, EngineError> {
        if rec.rehearsal || rec.backstop.as_ref().is_some_and(|b| b.installed) {
            return Ok(Ok(()));
        }
        let host = match self.backstop_host(rec) {
            Ok(Some(h)) => h,
            Ok(None) => return Ok(Ok(())),
            Err(why) => return Ok(Err(why)),
        };
        // The artifact lives in the instance directory, so the directory
        // comes first: on a run-capable host that is R0407 when the target
        // is not bootstrapped.
        if let Err(why) = self.ensure_instance_dir(rec, &host)? {
            return Ok(Err(why));
        }
        let (file_name, text) = match self.render_artifact(rec, &host) {
            Ok(a) => a,
            Err(why) => return Ok(Err(why)),
        };
        let sched_name = match &host.scheduler {
            Some(s) => s.clone(),
            None => {
                return Ok(Err(format!(
                    "R0401: {} declares no scheduler, and the plan's backstop is on it",
                    host.name()
                )))
            }
        };
        let mut st = BackstopState {
            host: host.name().to_string(),
            artifact: file_name.to_string(),
            scheduler: sched_name.clone(),
            installed: false,
            armed: false,
            deadline: None,
            heartbeat_s: None,
            interval_s: None,
            last_heartbeat: None,
            confirmed: false,
            fired: false,
        };
        let job = self.job_of(rec, &host, &st);
        // The artifact itself goes through the executor; the entry through
        // the scheduler binding. Both are borrowed from disjoint fields.
        let id = rec.id.clone();
        let Some(i) = self.executor_index(&host) else {
            return Ok(Err(format!("no transport reaches {}", host.name())));
        };
        let ex = &mut self.executors[i];
        if let Err(e) = ex.put_file(&host, &id, file_name, text.as_bytes(), 0o750) {
            return Ok(Err(format!(
                "the backstop artifact on {}: {e}",
                host.name()
            )));
        }
        let Some(sched) = self.schedulers.iter_mut().find(|s| s.name() == sched_name) else {
            return Ok(Err(format!(
                "R0401: no scheduler binding named {sched_name} for {}",
                host.name()
            )));
        };
        if let Err(e) = sched.install(ex.as_mut(), &host, &job) {
            return Ok(Err(format!(
                "R0401: the scheduler entry on {}: {e}",
                host.name()
            )));
        }
        match sched.present(ex.as_mut(), &host, &job) {
            Ok(Presence::Absent) => {
                return Ok(Err(format!(
                    "R0401: {sched_name} on {} reports no entry after installing it",
                    host.name()
                )))
            }
            Err(e) => return Ok(Err(format!("R0401: {sched_name} on {}: {e}", host.name()))),
            Ok(_) => {}
        }
        st.installed = true;
        rec.backstop = Some(st);
        self.persist(rec)?;
        Ok(Ok(()))
    }

    /// Arm: the modes, the skew probe, then the deadline and heartbeat the
    /// artifact reads (R0403, R0406).
    pub(crate) fn backstop_arm(
        &mut self,
        rec: &mut InstanceRecord,
    ) -> Result<Result<(), String>, EngineError> {
        if rec.rehearsal {
            return Ok(Ok(()));
        }
        let Some(st) = rec.backstop.clone() else {
            return Ok(Ok(()));
        };
        if st.armed || !st.installed {
            return Ok(Ok(()));
        }
        let Some(host) = self.host_of(&st.host) else {
            return Ok(Err(format!("{} is not a host of the site", st.host)));
        };
        let now = self.clock.now();
        let sched = schedule(
            &rec.plan()
                .backstop
                .as_ref()
                .map(|b| b.triggers.clone())
                .unwrap_or_default(),
            now,
            rec.deadline,
        );
        let job = self.job_of(rec, &host, &st);
        let id = rec.id.clone();
        let tolerance = self.skew_tolerance;
        let Some(i) = self.executor_index(&host) else {
            return Ok(Err(format!("no transport reaches {}", host.name())));
        };
        let ex = &mut self.executors[i];
        // R0406: the directory the artifact reads must carry its modes.
        match ex.instance_dir_list(&host) {
            Ok(dirs) => {
                if let Some(d) = dirs.iter().find(|d| d.instance == id) {
                    if !d.modes_ok {
                        return Ok(Err(format!(
                            "R0406: the instance directory of {id} on {} has the wrong modes; arming refused",
                            host.name()
                        )));
                    }
                }
            }
            Err(e) => {
                return Ok(Err(format!(
                    "the instance directories on {}: {e}",
                    host.name()
                )))
            }
        }
        // R0403: the target's clock against the engine's.
        match ex.clock_now(&host) {
            Ok(Some(target)) => {
                let d = skew(now, target);
                if d > tolerance.seconds {
                    return Ok(Err(format!(
                        "R0403: {} is {d}s from the controller's clock, beyond the {}s tolerance; arming refused",
                        host.name(),
                        tolerance.seconds
                    )));
                }
            }
            Ok(None) => {}
            Err(e) => {
                return Ok(Err(format!(
                    "R0403: the clock probe on {}: {e}",
                    host.name()
                )))
            }
        }
        if let Some(d) = sched.deadline {
            if let Err(e) = ex.replace_file(&host, &id, "deadline", &epoch_text(d)) {
                return Ok(Err(format!("the deadline on {}: {e}", host.name())));
            }
        }
        if sched.heartbeat_s.is_some() {
            if let Err(e) = ex.replace_file(&host, &id, "heartbeat", &epoch_text(now)) {
                return Ok(Err(format!("the heartbeat on {}: {e}", host.name())));
            }
        }
        let Some(s) = self
            .schedulers
            .iter_mut()
            .find(|s| s.name() == st.scheduler)
        else {
            return Ok(Err(format!(
                "R0401: no scheduler binding named {} for {}",
                st.scheduler,
                host.name()
            )));
        };
        if let Err(e) = s.arm(ex.as_mut(), &host, &job, sched.deadline.unwrap_or(now)) {
            return Ok(Err(format!("R0401: arming on {}: {e}", host.name())));
        }
        let b = rec.backstop.as_mut().expect("the state was cloned from it");
        b.armed = true;
        b.deadline = sched.deadline;
        b.heartbeat_s = sched.heartbeat_s;
        b.interval_s = sched.interval_s;
        b.last_heartbeat = sched.heartbeat_s.map(|_| now);
        self.persist(rec)?;
        Ok(Ok(()))
    }

    /// Rearm at renewal: only the deadline changes, and it is written
    /// before the new expiry is the instance's (R0404).
    pub(crate) fn backstop_rearm(
        &mut self,
        rec: &mut InstanceRecord,
        deadline: Instant,
    ) -> Result<Result<(), String>, EngineError> {
        let Some(st) = rec.backstop.clone() else {
            return Ok(Ok(()));
        };
        if !st.armed || rec.rehearsal {
            return Ok(Ok(()));
        }
        let Some(host) = self.host_of(&st.host) else {
            return Ok(Err(format!("R0404: {} is not a host of the site", st.host)));
        };
        let job = self.job_of(rec, &host, &st);
        let id = rec.id.clone();
        let Some(i) = self.executor_index(&host) else {
            return Ok(Err(format!("R0404: no transport reaches {}", host.name())));
        };
        let ex = &mut self.executors[i];
        if let Err(e) = ex.replace_file(&host, &id, "deadline", &epoch_text(deadline)) {
            return Ok(Err(format!("R0404: the deadline on {}: {e}", host.name())));
        }
        let Some(s) = self
            .schedulers
            .iter_mut()
            .find(|s| s.name() == st.scheduler)
        else {
            return Ok(Err(format!(
                "R0404: no scheduler binding named {}",
                st.scheduler
            )));
        };
        if let Err(e) = s.rearm(ex.as_mut(), &host, &job, deadline) {
            return Ok(Err(format!("R0404: rearming on {}: {e}", host.name())));
        }
        if let Some(b) = rec.backstop.as_mut() {
            b.deadline = Some(deadline);
        }
        self.persist(rec)?;
        Ok(Ok(()))
    }

    /// Disarm, on `confirm()` (the `unless_confirmed` trigger) or on
    /// `commit()`, `abandon` and a clean revert (the entry itself).
    pub(crate) fn backstop_disarm(
        &mut self,
        rec: &mut InstanceRecord,
        which: Disarm,
    ) -> Result<Result<(), String>, EngineError> {
        let Some(st) = rec.backstop.clone() else {
            return Ok(Ok(()));
        };
        if rec.rehearsal || !st.installed {
            return Ok(Ok(()));
        }
        let triggers = rec
            .plan()
            .backstop
            .as_ref()
            .map(|b| b.triggers.clone())
            .unwrap_or_default();
        let has_heartbeat = triggers
            .iter()
            .any(|t| matches!(t, Trigger::UnlessHeartbeat { .. }));
        // A confirm with a heartbeat still to serve takes the deadline
        // away and leaves the entry; with nothing else to serve it is the
        // whole disarm.
        let whole = matches!(which, Disarm::All) || !has_heartbeat;
        let Some(host) = self.host_of(&st.host) else {
            return Ok(Err(format!("{} is not a host of the site", st.host)));
        };
        let job = self.job_of(rec, &host, &st);
        let id = rec.id.clone();
        let Some(i) = self.executor_index(&host) else {
            return Ok(Err(format!("no transport reaches {}", host.name())));
        };
        let ex = &mut self.executors[i];
        if let Err(e) = ex.remove_file(&host, &id, "deadline") {
            return Ok(Err(format!("the deadline on {}: {e}", host.name())));
        }
        if whole {
            let Some(s) = self
                .schedulers
                .iter_mut()
                .find(|s| s.name() == st.scheduler)
            else {
                return Ok(Err(format!("no scheduler binding named {}", st.scheduler)));
            };
            if let Err(e) = s.disarm(ex.as_mut(), &host, &job) {
                return Ok(Err(format!("disarming on {}: {e}", host.name())));
            }
            if let Err(e) = ex.remove_file(&host, &id, &st.artifact) {
                return Ok(Err(format!("the artifact on {}: {e}", host.name())));
            }
        }
        if let Some(b) = rec.backstop.as_mut() {
            b.confirmed = true;
            if whole {
                b.armed = false;
                b.installed = false;
            }
        }
        self.persist(rec)?;
        Ok(Ok(()))
    }

    /// A backstop that fires after its instance was abandoned: accepted
    /// and visible (7.7). The instance is closed, so the reap pass looks
    /// only for the marker and journals what the target undid.
    pub(crate) fn backstop_fired_after_abandon(
        &mut self,
        rec: &mut InstanceRecord,
    ) -> Result<bool, EngineError> {
        let Some(st) = rec.backstop.clone() else {
            return Ok(false);
        };
        if !st.installed || !st.armed || st.fired {
            return Ok(false);
        }
        let Some(host) = self.host_of(&st.host) else {
            return Ok(false);
        };
        let id = rec.id.clone();
        let steps: Vec<u32> = coverage(rec.plan()).map(|c| c.covered).unwrap_or_default();
        let Some(i) = self.executor_index(&host) else {
            return Ok(false);
        };
        if self.executors[i].get_file(&host, &id, "fired").is_err() {
            return Ok(false);
        }
        self.log(
            rec,
            J::BackstopFiredAfterAbandon {
                host: host.name().to_string(),
                steps,
            },
        )?;
        if let Some(b) = rec.backstop.as_mut() {
            b.fired = true;
            b.armed = false;
        }
        self.persist(rec)?;
        Ok(true)
    }

    /// Instance directories a host holds that the store does not know and
    /// that are armed: what reconciliation left in place, for `rue doctor`.
    pub(crate) fn orphans(&mut self) -> Result<Unclaimed, EngineError> {
        let known: Vec<String> = self
            .instances()?
            .into_iter()
            .filter(|r| !rue_core::states::terminal(r.state))
            .map(|r| r.id)
            .collect();
        let hosts: Vec<Host> = self.hosts.values().cloned().collect();
        let ours = self.store.controller().to_string();
        let mut v = Vec::new();
        let mut foreign = Vec::new();
        for host in hosts {
            let Some(i) = self.executor_index(&host) else {
                continue;
            };
            let Ok(dirs) = self.executors[i].instance_dir_list(&host) else {
                continue;
            };
            for d in dirs {
                if known.contains(&d.instance) {
                    continue;
                }
                if crate::lifecycle::foreign_controller(
                    self.executors[i].as_mut(),
                    &host,
                    &d.instance,
                    &ours,
                )
                .is_some()
                {
                    foreign.push((host.name().to_string(), d.instance));
                    continue;
                }
                if d.armed && !d.fired {
                    v.push((host.name().to_string(), d.instance));
                }
            }
        }
        Ok((v, foreign))
    }

    /// One heartbeat pass: every armed instance whose interval has elapsed
    /// gets a fresh `heartbeat`. An engine that stops writing them is what
    /// the trigger notices.
    pub fn heartbeat(&mut self) -> Result<Vec<String>, EngineError> {
        let now = self.clock.now();
        let mut written = Vec::new();
        for mut rec in self.instances()? {
            let Some(st) = rec.backstop.clone() else {
                continue;
            };
            let (Some(interval), true) = (st.interval_s, st.armed) else {
                continue;
            };
            if st.fired {
                continue;
            }
            if let Some(last) = st.last_heartbeat {
                if now.unix_s < last.unix_s + interval {
                    continue;
                }
            }
            let Some(host) = self.host_of(&st.host) else {
                continue;
            };
            let id = rec.id.clone();
            let Some(i) = self.executor_index(&host) else {
                continue;
            };
            let ex = &mut self.executors[i];
            if ex
                .replace_file(&host, &id, "heartbeat", &epoch_text(now))
                .is_ok()
            {
                if let Some(b) = rec.backstop.as_mut() {
                    b.last_heartbeat = Some(now);
                }
                self.persist(&rec)?;
                written.push(id);
            }
        }
        Ok(written)
    }

    /// Read what a fired artifact left: the `fired` marker, which steps it
    /// undid (their markers are gone), and the drift it met. Journaled on
    /// the engine's next contact (R0402).
    pub(crate) fn backstop_read_fired(
        &mut self,
        rec: &mut InstanceRecord,
    ) -> Result<bool, EngineError> {
        let Some(st) = rec.backstop.clone() else {
            return Ok(false);
        };
        if !st.installed || st.fired || rec.rehearsal {
            return Ok(false);
        }
        let Some(host) = self.host_of(&st.host) else {
            return Ok(false);
        };
        let id = rec.id.clone();
        let covered: Vec<u32> = coverage(rec.plan()).map(|c| c.covered).unwrap_or_default();
        let applied = applied_steps(rec);
        let (fired, undone, drift, clobbered) = {
            let Some(i) = self.executor_index(&host) else {
                return Ok(false);
            };
            let ex = &mut self.executors[i];
            if ex.get_file(&host, &id, "fired").is_err() {
                return Ok(false);
            }
            let mut undone = Vec::new();
            for n in covered.iter().filter(|n| applied.contains(n)) {
                if ex.get_file(&host, &id, &format!("markers/{n}")).is_err() {
                    undone.push(*n);
                }
            }
            let drift = ex.get_file(&host, &id, "drift").is_ok();
            let clobbered = ex.get_file(&host, &id, "clobbered").is_ok();
            (true, undone, drift, clobbered)
        };
        if !fired {
            return Ok(false);
        }
        for n in &undone {
            self.log(rec, J::BackstopFired { step: *n })?;
        }
        if clobbered {
            self.log(
                rec,
                J::DriftClobbered {
                    step: undone.first().copied().unwrap_or(0),
                    facts: vec![format!("the artifact on {}", host.name())],
                },
            )?;
        }
        if drift {
            self.log(
                rec,
                J::DriftHeld {
                    step: undone.first().copied().unwrap_or(0),
                    facts: vec![format!("the artifact on {}", host.name())],
                },
            )?;
        }
        rec.applied.retain(|a| !undone.contains(&a.step));
        if let Some(b) = rec.backstop.as_mut() {
            b.fired = true;
            b.armed = false;
        }
        self.persist(rec)?;
        Ok(true)
    }

    /// The job an instance directory names on a host, without the store:
    /// the language and the root are the host's, and the artifact's name
    /// follows from the language (7.7).
    fn job_for(&self, host: &Host, instance: &str) -> Job {
        let shell = rue_core::artifact::shell_of(&host.record.os);
        let language = rue_core::artifact::language_of(&host.record);
        let file = match language {
            rue_core::model::ArtifactLanguage::Sh => "artifact.sh",
            rue_core::model::ArtifactLanguage::Powershell => "artifact.ps1",
            rue_core::model::ArtifactLanguage::Python => "artifact.py",
        };
        let root = RInstance {
            id: instance.to_string(),
            rue_root: host.rue_root.clone(),
        }
        .root(shell);
        let sep = if matches!(shell, rue_core::artifact::Shell::Powershell) {
            "\\"
        } else {
            "/"
        };
        Job {
            instance: instance.to_string(),
            artifact: format!("{root}{sep}instances{sep}{instance}{sep}{file}"),
            language,
            os: host.record.os.clone(),
        }
    }

    /// Whether the scheduler entry for an instance is there. `Unknown` is
    /// never read as absence.
    fn entry_presence(&mut self, host: &Host, instance: &str) -> Presence {
        let Some(name) = host.scheduler.clone() else {
            return Presence::Absent;
        };
        let job = self.job_for(host, instance);
        let Some(i) = self.executor_index(host) else {
            return Presence::Unknown;
        };
        let ex = &mut self.executors[i];
        let Some(s) = self.schedulers.iter_mut().find(|s| s.name() == name) else {
            return Presence::Unknown;
        };
        s.present(ex.as_mut(), host, &job)
            .unwrap_or(Presence::Unknown)
    }

    /// Reconciliation at boot (7.7): every instance directory on every
    /// reachable host against the store. A directory stamped by another
    /// controller is left exactly as it is and journaled
    /// `InstanceDirForeign`: it is not this store's to reclaim, and the
    /// instance it belongs to is live elsewhere. Of the rest, a directory
    /// the store does not know is left where it holds an armed, unfired
    /// artifact, journaled `InstanceDirOrphaned{armed: true}` and listed by
    /// `rue doctor`; one with no artifact or with a fired marker is
    /// reclaimed.
    pub fn reconcile(&mut self) -> Result<Reconciled, EngineError> {
        let known: Vec<String> = self
            .instances()?
            .into_iter()
            .filter(|r| !rue_core::states::terminal(r.state))
            .map(|r| r.id)
            .collect();
        let hosts: Vec<Host> = self.hosts.values().cloned().collect();
        let mut orphaned = Vec::new();
        let mut reclaimed = Vec::new();
        let mut foreign = Vec::new();
        for host in hosts {
            let Some(i) = self.executor_index(&host) else {
                continue;
            };
            if !self.executors[i].capabilities().filesystem {
                continue;
            }
            let Ok(dirs) = self.executors[i].instance_dir_list(&host) else {
                continue;
            };
            let ours = self.store.controller().to_string();
            for d in dirs {
                if known.contains(&d.instance) {
                    continue;
                }
                if let Some(other) = crate::lifecycle::foreign_controller(
                    self.executors[i].as_mut(),
                    &host,
                    &d.instance,
                    &ours,
                ) {
                    self.journal_site_event(J::InstanceDirForeign {
                        host: host.name().to_string(),
                        instance: d.instance.clone(),
                        controller: other,
                    })?;
                    foreign.push((host.name().to_string(), d.instance));
                    continue;
                }
                if d.armed && !d.fired {
                    self.journal_site_event(J::InstanceDirOrphaned {
                        host: host.name().to_string(),
                        instance: d.instance.clone(),
                        armed: true,
                    })?;
                    orphaned.push((host.name().to_string(), d.instance));
                    continue;
                }
                let Some(i) = self.executor_index(&host) else {
                    continue;
                };
                if self.executors[i]
                    .instance_dir_remove(&host, &d.instance)
                    .is_ok()
                {
                    self.journal_site_event(J::Reclaimed {
                        host: host.name().to_string(),
                        instance: d.instance.clone(),
                        forced: false,
                        reason: "boot reconciliation".into(),
                    })?;
                    reclaimed.push((host.name().to_string(), d.instance));
                }
            }
        }
        Ok((orphaned, reclaimed, foreign))
    }

    /// `rue reclaim`: remove an orphaned instance directory. Refused while
    /// the artifact is armed and its scheduler entry is present (R0405);
    /// `--force --reason` is accepted when the entry is absent or the
    /// operator states the artifact has been read.
    pub fn reclaim(
        &mut self,
        host_name: &str,
        instance: &str,
        force: bool,
        reason: &str,
    ) -> Result<String, EngineError> {
        let Some(host) = self.host_of(host_name) else {
            return Err(EngineError::Runtime(format!(
                "{host_name} is not a host of the site"
            )));
        };
        let Some(i) = self.executor_index(&host) else {
            return Err(EngineError::Runtime(format!(
                "no transport reaches {host_name}"
            )));
        };
        let dirs = self.executors[i].instance_dir_list(&host).map_err(|e| {
            EngineError::Runtime(format!("the instance directories on {host_name}: {e}"))
        })?;
        let Some(d) = dirs.into_iter().find(|d| d.instance == instance) else {
            return Err(EngineError::Runtime(format!(
                "{host_name} has no instance directory {instance}"
            )));
        };
        if d.armed && !d.fired {
            let present = self.entry_presence(&host, instance);
            if !force {
                return Err(EngineError::Runtime(format!(
                    "R0405: the artifact of {instance} on {host_name} is armed and its scheduler entry is {present}; reclaim it with --force --reason once the artifact has been read"
                )));
            }
            if reason.trim().is_empty() {
                return Err(EngineError::Runtime(
                    "R0405: --force needs a --reason".into(),
                ));
            }
        }
        let Some(i) = self.executor_index(&host) else {
            return Err(EngineError::Runtime(format!(
                "no transport reaches {host_name}"
            )));
        };
        self.executors[i]
            .instance_dir_remove(&host, instance)
            .map_err(|e| {
                EngineError::Runtime(format!("reclaiming {instance} on {host_name}: {e}"))
            })?;
        self.journal_site_event(J::Reclaimed {
            host: host_name.to_string(),
            instance: instance.to_string(),
            forced: force,
            reason: reason.to_string(),
        })?;
        Ok(format!("reclaimed {instance} on {host_name}"))
    }
}

/// What a canary found on one host (`rue doctor --canary`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Canary {
    pub host: String,
    /// The scheduler binding that was asked to fire it.
    pub scheduler: String,
    /// Whether the artifact actually ran and left its marker.
    pub fired: bool,
    /// Seconds from arming to the marker, when it fired.
    pub after_s: Option<u64>,
    /// What went wrong, when nothing fired.
    pub note: String,
}

impl Engine {
    /// `rue doctor --canary`: prove that a real backstop fires on every
    /// host that has a scheduler, by installing a throwaway artifact of
    /// the engine's own, arming it with a deadline already past, and
    /// waiting for the marker it leaves.
    ///
    /// Nothing of a plan is involved: the artifact undoes nothing and
    /// touches nothing outside its own instance directory, which is
    /// removed afterwards whatever happened. What it proves is the part
    /// no unit test can: that this host's scheduler runs what rue
    /// installs, at the granularity it claims.
    pub fn canary(&mut self, wait: Duration) -> Result<Vec<Canary>, EngineError> {
        let hosts: Vec<Host> = self.hosts.values().cloned().collect();
        let mut out = Vec::new();
        for host in hosts {
            let Some(scheduler) = host.scheduler.clone() else {
                continue;
            };
            let mut c = Canary {
                host: host.name().to_string(),
                scheduler: scheduler.clone(),
                fired: false,
                after_s: None,
                note: String::new(),
            };
            match self.canary_on(&host, &scheduler, wait, &mut c) {
                Ok(()) => {}
                Err(why) => c.note = why,
            }
            let _ = self.canary_clean(&host, &scheduler, &c);
            out.push(c);
        }
        Ok(out)
    }

    /// The instance a canary uses: named for what it is, and for this
    /// engine, so two canaries never collide.
    fn canary_id(&self) -> String {
        format!("rue-canary-{}", hex(&nonce()[..6]))
    }

    fn canary_on(
        &mut self,
        host: &Host,
        scheduler: &str,
        wait: Duration,
        c: &mut Canary,
    ) -> Result<(), String> {
        let id = self.canary_id();
        c.note = id.clone();
        let shell = rue_core::artifact::shell_of(&host.record.os);
        if matches!(shell, rue_core::artifact::Shell::Powershell) {
            return Err("a canary on Windows is Phase 3W's; no artifact is installed".into());
        }
        let root = RInstance {
            id: id.clone(),
            rue_root: host.rue_root.clone(),
        }
        .root(shell);
        let inst = format!("{root}/instances/{id}");
        // The whole artifact: it leaves a marker and stops. An artifact
        // that fires twice is no worse than one that fires once.
        let text = format!("#!/bin/sh\n# rue canary: proves this host's scheduler runs what rue installs.\n[ -e '{inst}/fired' ] && exit 0\nnow=$(date +%s)\nd=$(cat '{inst}/deadline' 2>/dev/null || echo 0)\n[ \"$now\" -ge \"$d\" ] || exit 0\n: > '{inst}/fired'\nexit 0\n");
        let Some(i) = self.executor_index(host) else {
            return Err(format!("no transport reaches {}", host.name()));
        };
        match self.executors[i].bootstrap_state(host) {
            Ok(s) if s.ready() => {}
            Ok(_) => return Err(format!("R0407: {} is not bootstrapped", host.name())),
            Err(e) => return Err(format!("{}: {e}", host.name())),
        }
        self.executors[i]
            .instance_dir_create(host, &id)
            .map_err(|e| format!("the canary's directory: {e}"))?;
        self.executors[i]
            .put_file(host, &id, "artifact.sh", text.as_bytes(), 0o750)
            .map_err(|e| format!("the canary's artifact: {e}"))?;
        let job = Job {
            instance: id.clone(),
            artifact: format!("{inst}/artifact.sh"),
            language: rue_core::model::ArtifactLanguage::Sh,
            os: host.record.os.clone(),
        };
        let Some(s) = self.schedulers.iter_mut().find(|s| s.name() == scheduler) else {
            return Err(format!("R0401: no scheduler binding named {scheduler}"));
        };
        let ex = &mut self.executors[i];
        s.install(ex.as_mut(), host, &job)
            .map_err(|e| format!("R0401: the canary's entry: {e}"))?;
        // Armed with a deadline already past: the next tick is the proof.
        let armed = self.clock.now();
        let Some(i) = self.executor_index(host) else {
            return Err(format!("no transport reaches {}", host.name()));
        };
        self.executors[i]
            .replace_file(
                host,
                &id,
                "deadline",
                format!("{}\n", armed.unix_s).as_bytes(),
            )
            .map_err(|e| format!("the canary's deadline: {e}"))?;
        let start = std::time::Instant::now();
        let bound = std::time::Duration::from_secs(wait.seconds);
        while start.elapsed() < bound {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let Some(i) = self.executor_index(host) else {
                break;
            };
            if self.executors[i].get_file(host, &id, "fired").is_ok() {
                c.fired = true;
                c.after_s = Some(start.elapsed().as_secs());
                c.note = format!("{id} fired");
                return Ok(());
            }
        }
        Err(format!(
            "{id} did not fire within {}s; {scheduler} on {} runs nothing rue installs",
            wait.seconds,
            host.name()
        ))
    }

    /// Whatever happened, the canary leaves nothing behind.
    fn canary_clean(&mut self, host: &Host, scheduler: &str, c: &Canary) -> Result<(), String> {
        let id = c
            .note
            .split_whitespace()
            .next()
            .filter(|s| s.starts_with("rue-canary-"))
            .unwrap_or_default()
            .to_string();
        if id.is_empty() {
            return Ok(());
        }
        let job = Job {
            instance: id.clone(),
            artifact: String::new(),
            language: rue_core::model::ArtifactLanguage::Sh,
            os: host.record.os.clone(),
        };
        let Some(i) = self.executor_index(host) else {
            return Ok(());
        };
        if let Some(s) = self.schedulers.iter_mut().find(|s| s.name() == scheduler) {
            let ex = &mut self.executors[i];
            let _ = s.disarm(ex.as_mut(), host, &job);
        }
        let _ = self.executors[i].instance_dir_remove(host, &id);
        Ok(())
    }
}
