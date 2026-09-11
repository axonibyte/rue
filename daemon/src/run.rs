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

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rue_bindings::secrets::{FileSource, Hold, Requester};
use rue_engine::clock::SystemClock;
use rue_engine::control::{
    self, Daemon, Operator, Operators, RegistrarDecl, SubscriberSink, Subscribers, UserSpec,
};
use rue_engine::executor::Executor;
use rue_engine::gates::Approval;
use rue_engine::hook::{
    hook_inventory, spawn_stdio_hook, HookAcceptor, HookApproval, HookExecutor, HookNotify,
    HookRegistry, HookScheduler, HookSink, HookSource, Registered,
};
use rue_engine::host::Host;
use rue_engine::journal::{Journal, Sink};
use rue_engine::lifecycle::Engine;
use rue_engine::notify::Notify;
/// The numeric uid where the platform has one, for the journal's account
/// line; Windows names an account and has none.
#[cfg(unix)]
fn my_uid_opt() -> Option<u32> {
    Some(rue_engine::peer::my_uid())
}

#[cfg(windows)]
fn my_uid_opt() -> Option<u32> {
    None
}

use rue_engine::scheduler::Scheduler;
use rue_engine::secrets::{Acceptor, Mailbox, Source};
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
    /// `--inventory`: a record to hold hosts from instead of asking an
    /// `inventory from: hook()`. Daemon dry-run mode rehearses against one;
    /// a live daemon asks the hook.
    pub inventory: Option<PathBuf>,
}

#[derive(Debug)]
pub enum Refusal {
    Usage(String),
    Refused(String),
}

fn refused(m: impl Into<String>) -> Refusal {
    Refusal::Refused(m.into())
}

/// A group by name or number.
/// The control channel's group must exist before the daemon serves: on
/// unix it owns the socket, on Windows it is named in the pipe's
/// access-control list.
#[cfg(unix)]
fn check_group(spec: &str) -> Result<(), Refusal> {
    match rue_engine::peer::gid_for(spec) {
        Some(_) => Ok(()),
        None => Err(refused(format!(
            "group {spec} does not exist; the control socket belongs to group rue (7.4), or name another with --group"
        ))),
    }
}

#[cfg(windows)]
fn check_group(spec: &str) -> Result<(), Refusal> {
    rue_engine::pipe::sddl(spec).map(|_| ()).map_err(refused)
}

fn sinks_of(
    decl: &SiteDecl,
    dir: &Path,
    hooks: &Arc<HookRegistry>,
    deadline: Duration,
    subscribers: &Arc<Subscribers>,
) -> Result<Vec<Box<dyn Sink>>, Refusal> {
    let mut sinks: Vec<Box<dyn Sink>> = vec![Box::new(SubscriberSink(subscribers.clone()))];
    for b in &decl.journal {
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
                rue_root: c.and_then(|c| c.rue_root.clone()),
                facts,
            }
        })
        .collect()
}

/// The `approval via:` binding of the site block. `always()` opens every
/// gate without a proof, so a live daemon refuses to build it: it exists
/// for daemon dry-run mode, where nothing is reserved and no executor is
/// called (7.9).
fn approval_of(
    decl: &SiteDecl,
    hooks: &Arc<HookRegistry>,
    deadline: Duration,
    dry_run: bool,
) -> Result<Option<Box<dyn Approval>>, Refusal> {
    let Some(b) = &decl.approval else {
        return Ok(None);
    };
    match b.kind.as_str() {
        "always" if !dry_run => Err(refused(
            "approval via: always() opens every gate without a proof; rued admits it only with --dry-run",
        )),
        "always" => Ok(Some(Box::new(rue_bindings::approval::Always))),
        "hook" => Ok(Some(Box::new(HookApproval {
            name: b.arg.clone().unwrap_or_default(),
            registry: hooks.clone(),
            deadline,
        }))),
        other => Err(refused(format!(
            "approval via: {other}() is not an approval binding"
        ))),
    }
}

/// The `secrets deliver_to:` acceptors, in the order the site declares
/// them: the first that accepts ends the delivery (5.13). The
/// `requester()` handle is returned too, because the control handler
/// drains what it took into the reply of the verb that produced it.
fn acceptors_of(
    decl: &SiteDecl,
    hooks: &Arc<HookRegistry>,
    deadline: Duration,
    mailbox: &Mailbox,
) -> Result<Vec<Box<dyn Acceptor>>, Refusal> {
    let requester = Requester::new(mailbox.clone());
    let mut v: Vec<Box<dyn Acceptor>> = Vec::new();
    for b in &decl.deliver_to {
        match b.kind.as_str() {
            "requester" => v.push(Box::new(requester.clone())),
            "hold" => {
                // `until:` is a duration or `:wane`; the engine resolves
                // `:wane` per instance (R0104 where it cannot).
                let d = b
                    .kws
                    .iter()
                    .find(|(k, _)| k == "until")
                    .and_then(|(_, val)| val.trim_end_matches('s').parse::<u64>().ok())
                    .map(rue_core::model::Duration::new);
                v.push(Box::new(Hold::new(d)));
            }
            "hook" => v.push(Box::new(HookAcceptor {
                name: b.arg.clone().unwrap_or_default(),
                registry: hooks.clone(),
                deadline,
            })),
            other => {
                return Err(refused(format!(
                    "secrets deliver_to: {other}() is not an acceptor"
                )))
            }
        }
    }
    Ok(v)
}

/// Every hook the daemon needs before it can serve its socket, and so
/// before anything can register over that socket.
///
/// Two bindings are used at boot: the inventory is asked once for the host
/// map, and the journal is written to by boot recovery itself. A hook
/// serving either must therefore be a `--spawn` child; one that means to
/// connect over the socket will not have registered yet, and the daemon
/// cannot wait for it because it is not listening.
///
/// This is checked before boot so the refusal names the problem. Without
/// it the failure arrives as `R0304: entry 1 not acknowledged by
/// hook(:x)`, which is true, unhelpful, and reads like a fault in the
/// hook rather than in how it was launched.
fn boot_time_hooks(decl: &SiteDecl) -> Vec<(&'static str, String)> {
    let mut need = Vec::new();
    if let Some(b) = &decl.inventory {
        if b.kind == "hook" {
            need.push(("inventory from", b.arg.clone().unwrap_or_default()));
        }
    }
    for b in &decl.journal {
        if b.kind == "hook" {
            need.push(("journal to", b.arg.clone().unwrap_or_default()));
        }
    }
    need
}

/// The hook an `inventory from: hook()` names, when the daemon must ask
/// it: `--inventory` names a record instead and takes precedence, which is
/// how dry-run mode rehearses a hook-inventoried site with no hook.
fn hook_inventory_name(decl: &SiteDecl, named_a_record: bool) -> Option<String> {
    if named_a_record {
        return None;
    }
    let b = decl.inventory.as_ref()?;
    (b.kind == "hook").then(|| b.arg.clone().unwrap_or_default())
}

/// The `secrets from:` binding of the site block: where a `secret(:ref)`
/// in a body is resolved, just before the body that names it runs. A site
/// with none declared refuses any step that names a secret, and says so.
fn source_of(
    decl: &SiteDecl,
    dir: &Path,
    hooks: &Arc<HookRegistry>,
    deadline: Duration,
) -> Result<Option<Box<dyn Source>>, Refusal> {
    let Some(b) = &decl.secrets_from else {
        return Ok(None);
    };
    match b.kind.as_str() {
        // Relative to the declaring file, as `inventory from: file()` is.
        "file" => {
            let rel = b
                .arg
                .clone()
                .ok_or_else(|| refused("secrets from: file() needs a path"))?;
            Ok(Some(Box::new(FileSource::new(dir.join(rel)))))
        }
        "hook" => Ok(Some(Box::new(HookSource {
            name: b.arg.clone().unwrap_or_default(),
            registry: hooks.clone(),
            deadline,
        }))),
        other => Err(refused(format!(
            "secrets from: {other}() is not a secret source"
        ))),
    }
}

/// The `notify via:` binding of the site block.
fn notify_of(
    decl: &SiteDecl,
    hooks: &Arc<HookRegistry>,
    deadline: Duration,
) -> Result<Option<Box<dyn Notify>>, Refusal> {
    let Some(b) = &decl.notify else {
        return Ok(None);
    };
    match b.kind.as_str() {
        "stdout" => Ok(Some(Box::new(rue_bindings::notify::Stdout))),
        "hook" => Ok(Some(Box::new(HookNotify {
            name: b.arg.clone().unwrap_or_default(),
            registry: hooks.clone(),
            deadline,
        }))),
        other => Err(refused(format!(
            "notify via: {other}() is not a notify binding"
        ))),
    }
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
        dry_run,
    }
}

/// Spawn a child hook: its stdout is read for its `register` frame and
/// then for replies; its stdin carries the engine's requests.
fn spawn_child(spec: &str, daemon: &Arc<Daemon>) -> Result<(), Refusal> {
    let (name, command) = spec
        .split_once('=')
        .ok_or_else(|| Refusal::Usage(format!("--spawn takes NAME=COMMAND, not {spec}")))?;
    // The spawn and the registration frame are the protocol's, and
    // `rue sdk-conform` performs the identical handshake; what is the
    // daemon's alone is who may register (R0505) and the journal.
    let mut hook = spawn_stdio_hook(name, command).map_err(|e| refused(e.to_string()))?;
    let reg = hook.registration.clone();
    let pid = hook.pid();
    // The child is the socket owner by construction; it must still be a
    // declared registrar's hook (R0505).
    let peer = control::Peer {
        user: rue_engine::peer::my_account(),
        owner: true,
        uid: my_uid_opt(),
    };
    let registrar = daemon
        .operators
        .registrar_for(&peer, &reg.name)
        .map_err(|e| refused(format!("{name}: {e}")))?;
    let link = hook.link.clone();
    daemon.hooks.register(Registered {
        registration: reg.clone(),
        registrar: registrar.name.clone(),
        connection: format!("child pid {pid}"),
        link: link.clone(),
    });
    // Acknowledge first, then journal. A hook cannot answer anything until
    // it has been acknowledged -- its serve loop is still waiting on that
    // line -- so journaling first deadlocks the moment the site's own
    // `journal to:` is a hook: the entry is sent to a child that is waiting
    // for the ack that the entry is holding up.
    //
    // The registration is still never left unjournaled: if the journal
    // refuses it (R0304), the hook is deregistered again and the daemon
    // refuses to start, so a registered hook and a journaled registration
    // remain the same set.
    hook.acknowledge().map_err(|e| refused(e.to_string()))?;

    // The pump before anything is asked of the hook. It is what delivers
    // replies to whoever is waiting; send a request with no reader on the
    // child's stdout and the answer goes nowhere, which the engine can
    // only see as Silent. That matters from the very first entry now that
    // registering a hook is itself journaled and the journal may be a hook.
    let reader = hook
        .take_stdout()
        .ok_or_else(|| refused(format!("{name}: no stdout")))?;
    let d = daemon.clone();
    let hook_name = reg.name.clone();
    let registrar_name = registrar.name.clone();
    let pumping = link.clone();
    std::thread::spawn(move || {
        pumping.pump(reader);
        d.hooks.deregister(&hook_name);
        let _ = d.journal_site(rue_core::journal::Event::HookDeregistered {
            name: hook_name,
            registrar: registrar_name,
            reason: "child exited".into(),
        });
        let mut hook = hook;
        let _ = hook.wait();
    });

    // Journalled last, and undone if the journal refuses: a registered hook
    // and a journaled registration stay the same set (R0304).
    if let Err(e) = daemon.journal_site(rue_core::journal::Event::HookRegistered {
        name: reg.name.clone(),
        registrar: registrar.name.clone(),
        connection: format!("child pid {pid} (stdio, socket owner)"),
    }) {
        daemon.hooks.deregister(&reg.name);
        return Err(refused(format!(
            "{}: registering {} could not be journaled, so it is not registered",
            e, reg.name
        )));
    }
    Ok(())
}

pub fn run(cfg: Config) -> Result<(), Refusal> {
    run_until(cfg, Arc::new(AtomicBool::new(false)))
}

/// The daemon, stopping when `stop` is set. `rued run` never sets it; a
/// Windows service sets it from its control handler.
pub fn run_until(cfg: Config, stop: Arc<AtomicBool>) -> Result<(), Refusal> {
    let sb = match site_bindings_opts(&cfg.site, cfg.dry_run, cfg.inventory.as_deref()) {
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
    check_group(&cfg.group)?;
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
    if let Some(a) = approval_of(&sb.decl, &hooks, deadline, cfg.dry_run)? {
        engine.set_approval(a);
    }
    if let Some(n) = notify_of(&sb.decl, &hooks, deadline)? {
        engine.set_notify(n);
    }
    let site_dir = cfg
        .site
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if let Some(src) = source_of(&sb.decl, &site_dir, &hooks, deadline)? {
        engine.set_secret_source(src);
    }
    let mailbox = Mailbox::new();
    for a in acceptors_of(&sb.decl, &hooks, deadline, &mailbox)? {
        engine.add_acceptor(a);
    }
    if let Some(d) = sb.decl.skew_tolerance {
        engine.set_skew_tolerance(d);
    }
    let daemon = Arc::new(Daemon {
        engine: Mutex::new(engine),
        operators: operators_of(&sb.decl, cfg.dry_run),
        hooks: hooks.clone(),
        subscribers,
        hook_deadline: deadline,
        dry_run: cfg.dry_run,
        mailbox,
    });
    // The children first: an `inventory from: hook()` has no hosts until
    // its hook has registered and been asked, and boot recovery needs the
    // hosts to reconcile against.
    //
    // Those serving `journal to:` are spawned before the rest, because
    // registering any hook is itself journaled: spawn another first and its
    // registration goes to a sink that does not exist yet, and the daemon
    // refuses to start. Ordering it here rather than asking the operator to
    // get `--spawn` in the right order means there is no order to get wrong.
    // Every one of them goes first, not just the first one: a site with two
    // hook sinks has two entries that must be deliverable before anything
    // else registers, and all must acknowledge (5.10).
    let journal_hooks: Vec<String> = sb
        .decl
        .journal
        .iter()
        .filter(|b| b.kind == "hook")
        .map(|b| b.arg.clone().unwrap_or_default())
        .collect();
    let mut specs: Vec<&String> = cfg.spawn.iter().collect();
    specs.sort_by_key(|spec| {
        !journal_hooks
            .iter()
            .any(|h| spec.starts_with(&format!("{h}=")))
    });
    for spec in specs {
        spawn_child(spec, &daemon)?;
    }
    // Every hook a boot-time binding names must have registered by now,
    // which means it must have been a `--spawn` child.
    for (slot, name) in boot_time_hooks(&sb.decl) {
        if slot == "inventory from" && cfg.inventory.is_some() {
            continue;
        }
        if !hooks.names().iter().any(|n| n == &name) {
            return Err(refused(format!(
                "the site's `{slot}: hook(:{name})` is needed before the socket is served, and \
                 {name} has not registered. A hook serving the inventory or the journal must be \
                 a `--spawn` child: one that connects over the socket cannot have registered \
                 yet, because the daemon is not listening until after boot. Spawn it with \
                 `--spawn {name}=COMMAND`{}",
                if slot == "inventory from" {
                    ", or name a record with --inventory"
                } else {
                    ""
                }
            )));
        }
    }
    if let Some(name) = hook_inventory_name(&sb.decl, cfg.inventory.is_some()) {
        let hosts = hook_inventory(&hooks, &name, deadline).map_err(|e| {
            refused(format!(
                "the site takes its inventory from hook {name}, and asking it failed: {e}. A \
                 hook that lists the site's hosts must be a `--spawn` child, since nothing has \
                 registered over the socket before the daemon serves it; or name a record with \
                 --inventory"
            ))
        })?;
        eprintln!("rued: inventory from hook {name}: {} hosts", hosts.len());
        let mut e = daemon.engine.lock().unwrap_or_else(|e| e.into_inner());
        e.journal_site_event(rue_core::journal::Event::InventoryListed {
            hook: name.clone(),
            hosts: hosts.iter().map(|h| h.name().to_string()).collect(),
        })
        .map_err(|e| refused(e.to_string()))?;
        e.set_hosts(hosts);
    }
    let boot = {
        let mut e = daemon.engine.lock().unwrap_or_else(|e| e.into_inner());
        e.boot().map_err(|e| refused(e.to_string()))?
    };
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
    control::serve(&cfg.socket, Some(&cfg.group), daemon, stop).map_err(|e| refused(e.to_string()))
}
