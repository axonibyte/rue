//! `task_scheduler()`, the Windows scheduler binding of docs/ROADMAP.md
//! 7.3 and 7.7: one scheduled task per instance, named for it, invoking
//! the artifact every minute.
//!
//! Like `cron()`, the entry is periodic and the artifact compares the
//! `deadline` file the engine writes, so `arm` and `rearm` change nothing
//! about the task. A PowerShell artifact is invoked by file rather than by
//! execution policy's leave; the roadmap's `-EncodedCommand` form is what
//! `Job::command` would carry for a one-liner, and the artifact is not one.
//!
//! Built and tested against the fake transport. This phase runs it on no
//! real Windows machine (the Phase 3 amendment); wine has no Task
//! Scheduler.

use rue_core::model::{Instant, Tri};
use rue_engine::executor::{ExecError, Executor, ProbeRun, RPrim, Resolved};
use rue_engine::host::Host;
use rue_engine::scheduler::{Job, Presence, Scheduler};

#[derive(Debug, Default)]
pub struct TaskScheduler;

fn task_name(instance: &str) -> String {
    format!("rue-{instance}")
}

/// `schtasks` takes its arguments quoted for cmd: a double-quoted string
/// with embedded quotes doubled. A name or path that carries one is
/// refused rather than guessed at.
fn q(s: &str) -> Result<String, ExecError> {
    if s.contains('"') {
        return Err(ExecError::Unsupported(format!(
            "a scheduled task argument with a double quote: {s}"
        )));
    }
    Ok(format!("\"{s}\""))
}

fn run(cmd: String) -> RPrim {
    RPrim::Run {
        cmd: Resolved::plain(&cmd),
        env: Vec::new(),
        stdin: None,
    }
}

impl Scheduler for TaskScheduler {
    fn name(&self) -> &str {
        "task_scheduler"
    }

    fn install(&mut self, ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError> {
        let cmd = format!(
            "schtasks /Create /F /RU SYSTEM /SC MINUTE /MO 1 /TN {} /TR {}",
            q(&task_name(&job.instance))?,
            q(&job.command())?
        );
        ex.run(host, &job.instance, &[run(cmd)]).map(|_| ())
    }

    fn arm(
        &mut self,
        _ex: &mut dyn Executor,
        _host: &Host,
        _job: &Job,
        _deadline: Instant,
    ) -> Result<(), ExecError> {
        Ok(())
    }

    fn rearm(
        &mut self,
        _ex: &mut dyn Executor,
        _host: &Host,
        _job: &Job,
        _deadline: Instant,
    ) -> Result<(), ExecError> {
        Ok(())
    }

    fn disarm(&mut self, ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError> {
        let cmd = format!("schtasks /Delete /F /TN {}", q(&task_name(&job.instance))?);
        ex.run(host, &job.instance, &[run(cmd)]).map(|_| ())
    }

    fn present(
        &mut self,
        ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
    ) -> Result<Presence, ExecError> {
        let cmd = format!("schtasks /Query /TN {}", q(&task_name(&job.instance))?);
        let probe = ProbeRun {
            name: format!("scheduled task for {}", job.instance),
            body: vec![run(cmd)],
        };
        Ok(match ex.observe(host, &probe)?.as_tri() {
            Tri::Yes => Presence::Present,
            Tri::No => Presence::Absent,
            Tri::Unknown => Presence::Unknown,
        })
    }
}
