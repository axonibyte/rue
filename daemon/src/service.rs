//! `rued` as a Windows service, docs/ROADMAP.md 7.9 and 12.
//!
//! The service-control manager starts the executable with the arguments
//! the service was registered with, hands the process a control handler,
//! and expects it to say when it is running and when it has stopped.
//! Everything else the daemon does is the same code the `run` verb runs;
//! this module is the dispatcher, the handler, and the argument parsing
//! between them.
//!
//! Registered with `sc.exe create rue binPath= "…\rued.exe service --site
//! … --store … --socket \\.\pipe\rue"`. Phase 3 builds and unit-tests this
//! and runs it as a service on no machine: what wine can show is the
//! parsing and the shape, not the manager (docs/TESTING.md).

#![cfg(windows)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

use crate::run;

/// The name the service is registered under.
pub const SERVICE_NAME: &str = "rue";

/// The arguments a service's `binPath=` carries after the verb, as the
/// daemon's own configuration. Every flag `rued run` takes is taken here
/// too, and an unknown one is a refusal rather than a default.
pub fn parse_args(args: &[OsString]) -> Result<run::Config, String> {
    let mut cfg = run::Config {
        site: PathBuf::new(),
        store: PathBuf::new(),
        socket: PathBuf::from(r"\\.\pipe\rue"),
        group: "rue".to_string(),
        dry_run: false,
        reap_every: 5,
        hook_deadline: 30,
        spawn: Vec::new(),
        inventory: None,
    };
    let mut it = args.iter().map(|a| a.to_string_lossy().into_owned());
    // The first argument is the service's own name, as the manager gives
    // it; everything after it is ours.
    let _ = it.next();
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--site" => cfg.site = PathBuf::from(value()?),
            "--store" => cfg.store = PathBuf::from(value()?),
            "--socket" => cfg.socket = PathBuf::from(value()?),
            "--group" => cfg.group = value()?,
            "--dry-run" => cfg.dry_run = true,
            "--reap-every" => {
                cfg.reap_every = value()?
                    .parse()
                    .map_err(|_| "--reap-every takes seconds".to_string())?
            }
            "--hook-deadline" => {
                cfg.hook_deadline = value()?
                    .parse()
                    .map_err(|_| "--hook-deadline takes seconds".to_string())?
            }
            "--spawn" => cfg.spawn.push(value()?),
            "--inventory" => cfg.inventory = Some(PathBuf::from(value()?)),
            other => return Err(format!("{other} is not a flag rued takes")),
        }
    }
    if cfg.site.as_os_str().is_empty() || cfg.store.as_os_str().is_empty() {
        return Err("a service needs --site and --store".into());
    }
    Ok(cfg)
}

/// The status a service reports as it starts, runs and stops.
pub fn status(state: ServiceState, accept: ServiceControlAccept) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    }
}

/// What the control handler does with each control the manager sends: it
/// answers `Interrogate`, sets the stop flag on `Stop` and `Shutdown`, and
/// tells the manager it does not implement anything else.
pub fn on_control(control: ServiceControl, stop: &Arc<AtomicBool>) -> ServiceControlHandlerResult {
    match control {
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stop.store(true, Ordering::SeqCst);
            ServiceControlHandlerResult::NoError
        }
        _ => ServiceControlHandlerResult::NotImplemented,
    }
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(args: Vec<OsString>) {
    if let Err(e) = serve(args) {
        eprintln!("rued: service: {e}");
    }
}

fn serve(args: Vec<OsString>) -> Result<(), String> {
    let cfg = parse_args(&args)?;
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = stop.clone();
    let handle = service_control_handler::register(SERVICE_NAME, move |control| {
        on_control(control, &handler_stop)
    })
    .map_err(|e| e.to_string())?;
    handle
        .set_service_status(status(
            ServiceState::Running,
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        ))
        .map_err(|e| e.to_string())?;
    let outcome = run::run_until(cfg, stop).map_err(|e| format!("{e:?}"));
    let _ = handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()));
    outcome
}

/// Hand the process to the service-control manager. Fails when the
/// process was not started by it, which is what running `rued service`
/// from a console does.
pub fn dispatch() -> Result<(), String> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<OsString> {
        std::iter::once(OsString::from(SERVICE_NAME))
            .chain(v.iter().map(OsString::from))
            .collect()
    }

    #[test]
    fn a_service_reads_the_flags_the_run_verb_takes_and_refuses_the_rest() {
        let cfg = parse_args(&args(&[
            "--site",
            r"C:\ProgramData\rue\site.rue",
            "--store",
            r"C:\ProgramData\rue\store",
            "--socket",
            r"\\.\pipe\rue",
            "--group",
            "rue-operators",
            "--reap-every",
            "9",
            "--hook-deadline",
            "45",
            "--spawn",
            "act=hook.exe",
            "--inventory",
            r"C:\ProgramData\rue\inventory.toml",
        ]))
        .unwrap();
        assert_eq!(cfg.store.to_string_lossy(), r"C:\ProgramData\rue\store");
        assert_eq!(cfg.socket.to_string_lossy(), r"\\.\pipe\rue");
        assert_eq!(cfg.group, "rue-operators");
        assert_eq!((cfg.reap_every, cfg.hook_deadline), (9, 45));
        assert_eq!(cfg.spawn, vec!["act=hook.exe".to_string()]);
        assert_eq!(
            cfg.inventory
                .as_deref()
                .map(|p| p.to_string_lossy().into_owned()),
            Some(r"C:\ProgramData\rue\inventory.toml".to_string())
        );
        assert!(!cfg.dry_run);
        // The pipe is the default channel, and the group is `rue`.
        let cfg = parse_args(&args(&["--site", "s.rue", "--store", "st"])).unwrap();
        assert_eq!(cfg.socket.to_string_lossy(), r"\\.\pipe\rue");
        assert_eq!(cfg.group, "rue");
        // A service with nowhere to keep its instances is a refusal.
        assert!(parse_args(&args(&["--site", "s.rue"])).is_err());
        // So is a flag rued does not take, and a flag with no value.
        assert!(parse_args(&args(&["--nonsense"])).is_err());
        assert!(parse_args(&args(&["--site"])).is_err());
    }

    #[test]
    fn the_control_handler_stops_on_stop_and_shutdown_and_says_what_it_does_not_serve() {
        let stop = Arc::new(AtomicBool::new(false));
        assert!(matches!(
            on_control(ServiceControl::Interrogate, &stop),
            ServiceControlHandlerResult::NoError
        ));
        assert!(!stop.load(Ordering::SeqCst), "interrogation stops nothing");
        assert!(
            matches!(
                on_control(ServiceControl::Pause, &stop),
                ServiceControlHandlerResult::NotImplemented
            ),
            "a control rued does not serve says so rather than pretending"
        );
        assert!(!stop.load(Ordering::SeqCst));
        assert!(matches!(
            on_control(ServiceControl::Stop, &stop),
            ServiceControlHandlerResult::NoError
        ));
        assert!(
            stop.load(Ordering::SeqCst),
            "stop sets the flag serve reads"
        );
        let stop = Arc::new(AtomicBool::new(false));
        assert!(matches!(
            on_control(ServiceControl::Shutdown, &stop),
            ServiceControlHandlerResult::NoError
        ));
        assert!(stop.load(Ordering::SeqCst));
    }

    #[test]
    fn the_status_it_reports_accepts_stop_while_running_and_nothing_when_stopped() {
        let running = status(
            ServiceState::Running,
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        );
        assert!(matches!(running.current_state, ServiceState::Running));
        assert!(running
            .controls_accepted
            .contains(ServiceControlAccept::STOP));
        let stopped = status(ServiceState::Stopped, ServiceControlAccept::empty());
        assert!(matches!(stopped.current_state, ServiceState::Stopped));
        assert!(stopped.controls_accepted.is_empty());
        assert!(matches!(stopped.exit_code, ServiceExitCode::Win32(0)));
    }
}
