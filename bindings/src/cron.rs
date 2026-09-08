//! `cron()`, the POSIX scheduler binding of docs/ROADMAP.md 7.3 and 7.7.
//!
//! The entry is a fenced region of the host's crontab, anchored by the
//! instance id, exactly as a `Region` fact is anchored elsewhere in rue:
//!
//! ```text
//! # rue-region <instance> begin
//! * * * * * /bin/sh /var/db/rue/instances/<instance>/artifact.sh
//! # rue-region <instance> end
//! ```
//!
//! The crontab is not a file fact an executor may write, so the edit is a
//! command: read it, strip this instance's region, append the region
//! again, install it. The whole edit runs under the host lock, so two
//! instances never race the same crontab.
//!
//! The artifact enforces its own deadline (it compares the `deadline` file
//! the engine writes), so the entry is periodic and `arm`/`rearm` have
//! nothing of their own to do. That is the shape 7.7 gives cron: "fires
//! within about one minute after the deadline".

use rue_core::model::{Instant, Tri};
use rue_engine::executor::{ExecError, Executor, ProbeRun, RPrim, Resolved};
use rue_engine::host::Host;
use rue_engine::scheduler::{Job, Presence, Scheduler};
use rue_render::quote;

/// The granularity of the entry: every minute.
const SCHEDULE: &str = "* * * * *";

#[derive(Debug, Default)]
pub struct Cron;

fn q(s: &str) -> Result<String, ExecError> {
    quote::posix(s).map_err(|e| ExecError::Unsupported(format!("cannot quote for sh: {e}")))
}

fn begin(instance: &str) -> String {
    format!("# rue-region {instance} begin")
}

fn end(instance: &str) -> String {
    format!("# rue-region {instance} end")
}

/// The command that reinstalls the crontab without this instance's region.
fn strip(instance: &str) -> Result<String, ExecError> {
    Ok(format!(
        "crontab -l 2>/dev/null | sed -e {}",
        q(&format!("/^{}$/,/^{}$/d", begin(instance), end(instance)))?
    ))
}

fn run(cmd: String) -> RPrim {
    RPrim::Run {
        cmd: Resolved::plain(&cmd),
        env: Vec::new(),
        stdin: None,
    }
}

impl Scheduler for Cron {
    fn name(&self) -> &str {
        "cron"
    }

    fn install(&mut self, ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError> {
        let _guard = ex.host_lock(host)?;
        let line = format!("{SCHEDULE} {}", job.command());
        let cmd = format!(
            "{{ {}; printf '%s\\n' {} {} {}; }} | crontab -",
            strip(&job.instance)?,
            q(&begin(&job.instance))?,
            q(&line)?,
            q(&end(&job.instance))?
        );
        ex.run(host, &job.instance, &[run(cmd)]).map(|_| ())
    }

    /// The deadline the artifact reads is the engine's to write; a
    /// periodic entry needs no change to become live.
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
        let _guard = ex.host_lock(host)?;
        let cmd = format!("{} | crontab -", strip(&job.instance)?);
        ex.run(host, &job.instance, &[run(cmd)]).map(|_| ())
    }

    fn present(
        &mut self,
        ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
    ) -> Result<Presence, ExecError> {
        let cmd = format!(
            "crontab -l 2>/dev/null | grep -q -e {}",
            q(&format!("^{}$", begin(&job.instance)))?
        );
        let probe = ProbeRun {
            name: format!("cron entry for {}", job.instance),
            body: vec![run(cmd)],
        };
        Ok(match ex.observe(host, &probe)?.as_tri() {
            Tri::Yes => Presence::Present,
            Tri::No => Presence::Absent,
            Tri::Unknown => Presence::Unknown,
        })
    }
}
