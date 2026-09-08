//! `launchd()`, the macOS scheduler binding of docs/ROADMAP.md 7.3.
//!
//! The entry is a launchd job whose property list lives beside the
//! artifact in the instance directory and which runs it every minute; the
//! artifact compares its own deadline, as under `cron()`.
//!
//! Shipped install-only and unexecuted: rue has no macOS controller and no
//! macOS target this phase (section 12 and the roadmap's later item for a
//! Mac to execute, sign and notarize). The commands are the ones a Mac
//! would run, tested against the fake transport, and `docs/TESTING.md`
//! records that nothing here has been executed.

use rue_core::model::{Instant, Tri};
use rue_engine::executor::{ExecError, Executor, ProbeRun, RPrim, Resolved};
use rue_engine::host::Host;
use rue_engine::scheduler::{Job, Presence, Scheduler};
use rue_render::quote;

#[derive(Debug, Default)]
pub struct Launchd;

fn q(s: &str) -> Result<String, ExecError> {
    quote::posix(s).map_err(|e| ExecError::Unsupported(format!("cannot quote for sh: {e}")))
}

fn label(instance: &str) -> String {
    format!("rue.{instance}")
}

fn plist(instance: &str, command: &str) -> String {
    let mut args = String::new();
    for word in command.split_whitespace() {
        args.push_str(&format!("    <string>{word}</string>\n"));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         <key>Label</key><string>{}</string>\n\
         <key>ProgramArguments</key><array>\n{args}</array>\n\
         <key>StartInterval</key><integer>60</integer>\n\
         </dict>\n\
         </plist>\n",
        label(instance)
    )
}

fn run(cmd: String) -> RPrim {
    RPrim::Run {
        cmd: Resolved::plain(&cmd),
        env: Vec::new(),
        stdin: None,
    }
}

impl Scheduler for Launchd {
    fn name(&self) -> &str {
        "launchd"
    }

    fn install(&mut self, ex: &mut dyn Executor, host: &Host, job: &Job) -> Result<(), ExecError> {
        let rel = format!("{}.plist", label(&job.instance));
        ex.put_file(
            host,
            &job.instance,
            &rel,
            plist(&job.instance, &job.command()).as_bytes(),
            0o640,
        )?;
        let path = job.artifact.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        let cmd = format!(
            "launchctl bootstrap system {}",
            q(&format!("{path}/{rel}"))?
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
        let cmd = format!("launchctl bootout system/{}", label(&job.instance));
        ex.run(host, &job.instance, &[run(cmd)])?;
        ex.remove_file(
            host,
            &job.instance,
            &format!("{}.plist", label(&job.instance)),
        )
    }

    fn present(
        &mut self,
        ex: &mut dyn Executor,
        host: &Host,
        job: &Job,
    ) -> Result<Presence, ExecError> {
        let cmd = format!("launchctl print system/{}", label(&job.instance));
        let probe = ProbeRun {
            name: format!("launchd job for {}", job.instance),
            body: vec![run(cmd)],
        };
        Ok(match ex.observe(host, &probe)?.as_tri() {
            Tri::Yes => Presence::Present,
            Tri::No => Presence::Absent,
            Tri::Unknown => Presence::Unknown,
        })
    }
}
