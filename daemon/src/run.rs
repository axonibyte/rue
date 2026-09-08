//! `rued run`: the site to a daemon.
//!
//! From the site block (7.3): the journal sinks (`local()` is the store's
//! own copy; `file(path)` relative to the site file; `stdout()`;
//! `hook(:x)` a sink that acknowledges when the registered hook does), the
//! signing key (`sign: key(path)`), the executors (`hook(:x, transport:
//! :t)`; `local()` and `ssh()` arrive with the executors unit, so a step
//! that needs them is deferred until then), the inventory (`file(path)`,
//! or `hook(:x)` listed from the hook at each request), the operators and
//! the registrars (7.4). A site with no operators block refuses to start
//! outside dry-run mode (E0604, which the resolver raises); dry-run mode
//! suspends it.
//!
//! Children (`--spawn NAME=COMMAND`) are started with their stdio as the
//! hook link; they are the socket owner by construction and must still be
//! a declared registrar's hook, which they say with a `register` frame on
//! their stdout like any other hook.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rue_engine::clock::SystemClock;
use rue_engine::control::{
    self, Daemon, Operator, Operators, RegistrarDecl, SubscriberSink, Subscribers, UserSpec,
};
use rue_engine::executor::Executor;
use rue_engine::hook::{
    HookExecutor, HookRegistry, HookScheduler, HookSink, LineLink, Registered, Registration,
    HOOK_PROTOCOL,
};
use rue_engine::host::Host;
use rue_engine::journal::{Journal, Sink};
use rue_engine::lifecycle::Engine;
use rue_engine::peer::my_uid;
use rue_engine::scheduler::Scheduler;
use rue_engine::sign::Signer;
use rue_engine::store::{schema_of, SchemaError, Store};
use rue_surface::resolve::site::SiteDecl;
use rue_surface::resolve::{site_bindings_opts, SiteBindings};

pub struct Config {
    pub site: PathBuf,
    pub store: PathBuf,
    pub socket: PathBuf,
    pub group: String,
    pub dry_run: bool,
    pub reap_every: u64,
    pub hook_deadline: u64,
    pub spawn: Vec<String>,
}

pub enum Refusal {
    Usage(String),
    Refused(String),
}

fn refused(m: impl Into<String>) -> Refusal {
    Refusal::Refused(m.into())
}

/// A group by name or number.
fn group_id(spec: &str) -> Result<u32, Refusal> {
    if let Ok(n) = spec.parse::<u32>() {
        return Ok(n);
    }
    let c = std::ffi::CString::new(spec).map_err(|_| Refusal::Usage("group name".into()))?;
    // SAFETY: getgrnam reads a NUL-terminated name and returns a static
    // entry or null.
    let g = unsafe { libc::getgrnam(c.as_ptr()) };
    if g.is_null() {
        return Err(refused(format!(
            "group {spec} does not exist; the control socket belongs to group rue (7.4), or name another with --group"
        )));
    }
    // SAFETY: g is a valid entry.
    Ok(unsafe { (*g).gr_gid })
}

fn sinks_of(
    decl: &SiteDecl,
    dir: &Path,
    hooks: &Arc<HookRegistry>,
    deadline: Duration,
    subscribers: &Arc<Subscribers>,
) -> Result<Vec<Box<dyn Sink>>, Refusal> {
    let mut sinks: Vec<Box<dyn Sink>> = vec![Box::new(SubscriberSink(subscribers.clone()))];
    if let Some(b) = &decl.journal {
        match b.kind.as_str() {
            "local" => {}
            "file" => {
                let p = dir.join(b.arg.clone().unwrap_or_default());
                sinks.push(Box::new(rue_bindings::journal::FileSink::new(&p)));
            }
            "stdout" => sinks.push(Box::new(rue_bindings::journal::StdoutSink)),
            "hook" => sinks.push(Box::new(HookSink {
                name: b.arg.clone().unwrap_or_default(),
                registry: hooks.clone(),
                deadline,
            })),
            other => return Err(refused(format!("journal to: {other}() is not a sink"))),
        }
    }
    Ok(sinks)
}

fn signer_of(decl: &SiteDecl, dir: &Path) -> Result<Option<Signer>, Refusal> {
    match &decl.journal_sign {
        Some(b) => {
            let p = dir.join(b.arg.clone().unwrap_or_default());
            Signer::load(&p).map(Some).map_err(refused)
        }
        None => Ok(None),
    }
}

fn hosts_of(sb: &SiteBindings) -> Vec<Host> {
    let sched = scheduler_name(&sb.decl);
    sb.inventory
        .hosts
        .iter()
        .map(|r| {
            let c = sb.inventory.contracts.iter().find(|c| c.name == r.name);
            let mut facts = std::collections::BTreeMap::new();
            if let Some(c) = c {
                if !c.roles.is_empty() {
                    facts.insert("roles".to_string(), c.roles.join(","));
                }
            }
            Host {
                record: r.clone(),
                address: c.map(|c| c.address.clone()).unwrap_or_default(),
                scheduler: if sb.inventory.scheduled.contains(&r.name) {
                    Some(sched.clone())
                } else {
                    None
                },
                rue_root: None,
                facts,
            }
        })
        .collect()
}

/// The `backstop scheduler:` binding of the site block. A site with none
/// declares no scheduler, and a plan with a `:target` backstop on a host
/// that names one is R0401 at apply.
fn schedulers_of(
    decl: &SiteDecl,
    hooks: &Arc<HookRegistry>,
    deadline: Duration,
    dry_run: bool,
) -> Result<Vec<Box<dyn Scheduler>>, Refusal> {
    let mut v: Vec<Box<dyn Scheduler>> = Vec::new();
    if dry_run {
        return Ok(v);
    }
    let Some(b) = &decl.scheduler else {
        return Ok(v);
    };
    match b.kind.as_str() {
        "cron" => v.push(Box::new(rue_bindings::cron::Cron)),
        "task_scheduler" => v.push(Box::new(rue_bindings::task_scheduler::TaskScheduler)),
        "launchd" => v.push(Box::new(rue_bindings::launchd::Launchd)),
        "hook" => v.push(Box::new(HookScheduler {
            name: b.arg.clone().unwrap_or_default(),
            registry: hooks.clone(),
            deadline,
        })),
        other => {
            return Err(refused(format!(
                "backstop scheduler: {other}() is not a scheduler"
            )))
        }
    }
    Ok(v)
}

/// The scheduler name a host record carries: the site's, for every host
/// the inventory says has one.
fn scheduler_name(decl: &SiteDecl) -> String {
    decl.scheduler
        .as_ref()
        .map(|b| match b.kind.as_str() {
            "hook" => b.arg.clone().unwrap_or_default(),
            k => k.to_string(),
        })
        .unwrap_or_else(|| "cron".into())
}

fn executors_of(
    decl: &SiteDecl,
    dir: &Path,
    hooks: &Arc<HookRegistry>,
    deadline: Duration,
    dry_run: bool,
) -> Result<Vec<Box<dyn Executor>>, Refusal> {
    if dry_run {
        return Ok(Vec::new());
    }
    let mut v: Vec<Box<dyn Executor>> = Vec::new();
    // The controller's own executor is always present for :controller steps.
    v.push(Box::new(rue_bindings::local::LocalExecutor::default()));
    for b in &decl.execute {
        let kw = |name: &str| {
            b.kws
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        match b.kind.as_str() {
            "hook" => v.push(Box::new(HookExecutor {
                name: b.arg.clone().unwrap_or_default(),
                transport: kw("transport").unwrap_or_default(),
                registry: hooks.clone(),
                deadline,
            })),
            "ssh" => {
                let identity =
                    dir.join(kw("identity").ok_or_else(|| refused("ssh() names its identity"))?);
                let known_hosts = dir
                    .join(kw("known_hosts").ok_or_else(|| refused("ssh() names its known_hosts"))?);
                if !identity.is_file() {
                    return Err(refused(format!(
                        "ssh() identity {} is not a file",
                        identity.display()
                    )));
                }
                let user = kw("user").unwrap_or_else(|| "root".into());
                v.push(Box::new(rue_bindings::ssh::SshExecutor::open_ssh(
                    identity,
                    known_hosts,
                    &user,
                )));
            }
            "local" => {}
            other => {
                return Err(refused(format!(
                    "execute via: {other}() is not an executor"
                )))
            }
        }
    }
    Ok(v)
}

fn operators_of(decl: &SiteDecl, dry_run: bool) -> Operators {
    Operators {
        identities: decl
            .identities
            .iter()
            .map(|i| Operator {
                name: i.name.clone(),
                user: UserSpec::parse(i.user.as_deref().unwrap_or("")),
                operator_for: i.operator_for.clone(),
                admin: i.admin,
                subscribe: i.subscribe.clone(),
            })
            .collect(),
        registrars: decl
            .registrars
            .iter()
            .map(|r| RegistrarDecl {
                name: r.name.clone(),
                user: UserSpec::parse(r.user.as_deref().unwrap_or("")),
                may_register: r.may_register.clone(),
            })
            .collect(),
        socket_owner_uid: my_uid(),
        dry_run,
    }
}

/// Spawn a child hook: its stdout is read for its `register` frame and
/// then for replies; its stdin carries the engine's requests.
fn spawn_child(spec: &str, daemon: &Arc<Daemon>) -> Result<(), Refusal> {
    let (name, command) = spec
        .split_once('=')
        .ok_or_else(|| Refusal::Usage(format!("--spawn takes NAME=COMMAND, not {spec}")))?;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| refused(format!("spawning {name}: {e}")))?;
    let stdin = child.stdin.take().ok_or_else(|| refused("child stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| refused("child stdout"))?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| refused(format!("{name}: reading its register frame: {e}")))?;
    let frame: serde_json::Value = serde_json::from_str(line.trim_end()).map_err(|e| {
        refused(format!(
            "{name}: its first line is not a register frame: {e}"
        ))
    })?;
    let reg: Registration = serde_json::from_value(
        frame
            .get("register")
            .cloned()
            .ok_or_else(|| refused(format!("{name}: its first frame must be a register")))?,
    )
    .map_err(|e| refused(format!("{name}: register: {e}")))?;
    if reg.protocol != HOOK_PROTOCOL {
        return Err(refused(format!(
            "{name}: hook protocol {} is not {HOOK_PROTOCOL}",
            reg.protocol
        )));
    }
    if reg.name != name {
        return Err(refused(format!("{name}: registers as {}", reg.name)));
    }
    // The child is the socket owner by construction; it must still be a
    // declared registrar's hook (R0505).
    let peer = control::Peer {
        cred: rue_engine::peer::PeerCred {
            uid: my_uid(),
            gid: 0,
        },
        user: rue_engine::peer::user_name(my_uid()),
        socket_owner_uid: my_uid(),
    };
    let registrar = daemon
        .operators
        .registrar_for(&peer, &reg.name)
        .map_err(|e| refused(format!("{name}: {e}")))?;
    let link = Arc::new(LineLink::new(&reg.name, Box::new(stdin)));
    daemon.hooks.register(Registered {
        registration: reg.clone(),
        registrar: registrar.name.clone(),
        connection: format!("child pid {}", child.id()),
        link: link.clone(),
    });
    daemon
        .journal_site(rue_core::journal::Event::HookRegistered {
            name: reg.name.clone(),
            registrar: registrar.name.clone(),
            connection: format!("child pid {} (stdio, socket owner)", child.id()),
        })
        .map_err(|e| refused(e.to_string()))?;
    // Acknowledge the registration on its stdin, as the socket does.
    {
        let mut ack = serde_json::to_vec(
            &serde_json::json!({ "register": { "ok": true, "name": reg.name } }),
        )
        .unwrap_or_default();
        ack.push(b'\n');
        let _ = link.call_raw_write(&ack);
    }
    let d = daemon.clone();
    let hook_name = reg.name.clone();
    let registrar_name = registrar.name;
    std::thread::spawn(move || {
        link.pump(Box::new(reader));
        d.hooks.deregister(&hook_name);
        let _ = d.journal_site(rue_core::journal::Event::HookDeregistered {
            name: hook_name,
            registrar: registrar_name,
            reason: "child exited".into(),
        });
        let _ = child.wait();
    });
    Ok(())
}

pub fn run(cfg: Config) -> Result<(), Refusal> {
    let sb = match site_bindings_opts(&cfg.site, cfg.dry_run) {
        Ok(sb) => sb,
        Err(diags) => {
            for d in &diags {
                eprintln!("rued: {}", d.render());
            }
            return Err(refused(format!(
                "the site block of {} does not validate",
                cfg.site.display()
            )));
        }
    };
    let gid = group_id(&cfg.group)?;
    let store = match schema_of(&cfg.store) {
        Err(SchemaError::Missing)
            if !cfg.store.exists()
                || std::fs::read_dir(&cfg.store)
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(false) =>
        {
            Store::create(&cfg.store).map_err(|e| refused(e.to_string()))?
        }
        _ => Store::open(&cfg.store).map_err(|e| refused(e.to_string()))?,
    };
    let hooks = Arc::new(HookRegistry::new());
    let subscribers = Arc::new(Subscribers::default());
    let deadline = Duration::from_secs(cfg.hook_deadline);
    let sinks = sinks_of(&sb.decl, &sb.dir, &hooks, deadline, &subscribers)?;
    let signer = signer_of(&sb.decl, &sb.dir)?;
    let journal = Journal::open(&store, sinks, signer).map_err(|e| refused(e.to_string()))?;
    let executors = executors_of(&sb.decl, &sb.dir, &hooks, deadline, cfg.dry_run)?;
    let hosts = hosts_of(&sb);
    let mut engine = Engine::open(store, journal, Arc::new(SystemClock), executors, hosts)
        .map_err(|e| refused(e.to_string()))?;
    for s in schedulers_of(&sb.decl, &hooks, deadline, cfg.dry_run)? {
        engine.add_scheduler(s);
    }
    if let Some(d) = sb.decl.skew_tolerance {
        engine.set_skew_tolerance(d);
    }
    let boot = engine.boot().map_err(|e| refused(e.to_string()))?;
    eprintln!(
        "rued: booted: {} demoted, {} reestablished, {} lost{}",
        boot.demoted.len(),
        boot.reestablished.len(),
        boot.lost.len(),
        boot.migrated
            .map(|(f, t)| format!(", migrated {f} -> {t}"))
            .unwrap_or_default()
    );
    if !boot.orphaned.is_empty() || !boot.reclaimed.is_empty() {
        eprintln!(
            "rued: reconciled: {} armed instance directories left in place, {} reclaimed",
            boot.orphaned.len(),
            boot.reclaimed.len()
        );
    }
    let daemon = Arc::new(Daemon {
        engine: Mutex::new(engine),
        operators: operators_of(&sb.decl, cfg.dry_run),
        hooks: hooks.clone(),
        subscribers,
        hook_deadline: deadline,
        dry_run: cfg.dry_run,
    });
    for spec in &cfg.spawn {
        spawn_child(spec, &daemon)?;
    }
    // The reap thread.
    {
        let d = daemon.clone();
        let every = Duration::from_secs(cfg.reap_every.max(1));
        std::thread::spawn(move || loop {
            std::thread::sleep(every);
            let report = {
                let mut e = d.engine.lock().unwrap_or_else(|e| e.into_inner());
                e.reap()
            };
            match report {
                Ok(r) => {
                    for a in r.actions {
                        eprintln!("rued: reap: {a}");
                    }
                    for (id, state) in r.notify {
                        eprintln!("rued: notify: {id} is {state}");
                    }
                }
                Err(e) => eprintln!("rued: reap: {e}"),
            }
        });
    }
    // The heartbeat thread: an armed `unless_heartbeat` backstop is told
    // the engine is alive at its interval, and an engine that stops is
    // what the trigger notices (5.6).
    {
        let d = daemon.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            let r = {
                let mut e = d.engine.lock().unwrap_or_else(|e| e.into_inner());
                e.heartbeat()
            };
            if let Err(e) = r {
                eprintln!("rued: heartbeat: {e}");
            }
        });
    }
    eprintln!(
        "rued: serving {} for site {} (store {}, {}{} hooks may register)",
        cfg.socket.display(),
        cfg.site.display(),
        cfg.store.display(),
        if cfg.dry_run { "dry-run, " } else { "" },
        daemon.operators.registrars.len()
    );
    let _ = std::io::stderr().flush();
    let stop = Arc::new(AtomicBool::new(false));
    control::serve(&cfg.socket, Some(gid), daemon, stop).map_err(|e| refused(e.to_string()))
}
