//! The executor seam, docs/ROADMAP.md section 7.2: everything the engine
//! does to a host goes through one object-safe trait, so a lifecycle runs
//! unchanged over `local()`, `ssh()`, a hook, or the fake below.
//!
//! The engine hands an executor a *resolved* body: every value already a
//! string with its secrecy known (`crate::resolve`), so an executor never
//! sees a reference and never decides where a value came from. `env:` and
//! `stdin:` of a run are carried on the primitive and honored by the stdin
//! preamble on executors whose capabilities say so (the checker refuses the
//! rest at E0211); the roadmap's `run_with_preamble` is that one path.
//!
//! An executor that returns empty output where an op promised one is
//! `ExecError::Silent`, which the engine treats as a refusal.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

use rue_core::model::Tri;
use serde::{Deserialize, Serialize};

use crate::host::Host;

/// Which locus an executor serves. The engine picks an executor by the
/// step's locus and the host's reach.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocusKind {
    Local,
    Ssh,
    Hook(String),
}

impl LocusKind {
    /// The transport name a host record's `reach` lists for this executor.
    pub fn transport(&self) -> &str {
        match self {
            LocusKind::Local => "local",
            LocusKind::Ssh => "ssh",
            LocusKind::Hook(t) => t,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecCaps {
    /// An instance directory can live on the target.
    pub filesystem: bool,
    /// `env:` and `stdin:` are delivered through the stdin preamble.
    pub stdin_preamble: bool,
}

/// A value after resolution: its text and whether it is a secret, which an
/// executor must keep off every argv, log and journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub text: String,
    pub secret: bool,
}

impl Resolved {
    pub fn plain(text: &str) -> Resolved {
        Resolved {
            text: text.to_string(),
            secret: false,
        }
    }
}

/// A resolved primitive: `rue_core::body::Prim` with every value a
/// [`Resolved`]. Paths and anchors come from the primitive's fact reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RPrim {
    Run {
        cmd: Resolved,
        env: Vec<(String, Resolved)>,
        stdin: Option<Resolved>,
    },
    Write {
        shape: String,
        content: Resolved,
    },
    Remove {
        shape: String,
    },
    Append {
        shape: String,
        line: Resolved,
    },
    RegionSet {
        shape: String,
        anchor: Option<String>,
        content: Resolved,
    },
    RegionClear {
        shape: String,
        anchor: Option<String>,
    },
    Stage {
        name: String,
        content: Resolved,
        mode: u32,
    },
    Hook {
        name: String,
        args: Vec<(String, Resolved)>,
    },
    Install {
        name: String,
    },
    Release {
        name: String,
    },
    /// A defprim call, already expanded to its run template.
    Call {
        prim: String,
        cmd: Resolved,
    },
}

impl RPrim {
    /// True when any value of the primitive is a secret.
    pub fn carries_secret(&self) -> bool {
        match self {
            RPrim::Run { cmd, env, stdin } => {
                cmd.secret
                    || env.iter().any(|(_, v)| v.secret)
                    || stdin.as_ref().is_some_and(|v| v.secret)
            }
            RPrim::Write { content, .. }
            | RPrim::RegionSet { content, .. }
            | RPrim::Stage { content, .. } => content.secret,
            RPrim::Append { line, .. } => line.secret,
            RPrim::Hook { args, .. } => args.iter().any(|(_, v)| v.secret),
            RPrim::Call { cmd, .. } => cmd.secret,
            RPrim::Remove { .. }
            | RPrim::RegionClear { .. }
            | RPrim::Install { .. }
            | RPrim::Release { .. } => false,
        }
    }
}

/// What a body produced: the text on stdout (never journaled) and the named
/// outputs the op declared, as the executor read them back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub stdout: String,
    pub outputs: BTreeMap<String, String>,
}

/// A probe's answer: its text, and the three-valued reading a guard takes
/// (`None` reads as `Unknown`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub text: String,
    pub tri: Option<Tri>,
}

impl Observation {
    pub fn yes(text: &str) -> Observation {
        Observation {
            text: text.to_string(),
            tri: Some(Tri::Yes),
        }
    }
    pub fn no(text: &str) -> Observation {
        Observation {
            text: text.to_string(),
            tri: Some(Tri::No),
        }
    }
    pub fn unknown(text: &str) -> Observation {
        Observation {
            text: text.to_string(),
            tri: Some(Tri::Unknown),
        }
    }
    pub fn as_tri(&self) -> Tri {
        self.tri.unwrap_or(Tri::Unknown)
    }
}

/// What `rue bootstrap` verifies (section 7.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapState {
    pub rue_root: bool,
    pub group: bool,
    pub instances_dir: bool,
    pub lock: bool,
    pub modes_ok: bool,
}

impl BootstrapState {
    pub fn ready(&self) -> bool {
        self.rue_root && self.group && self.instances_dir && self.lock && self.modes_ok
    }
}

/// One instance directory as a target reports it at reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceDirState {
    pub instance: String,
    pub armed: bool,
    pub fired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
    /// The body ran and failed; the text is what the executor may safely
    /// report (never a secret).
    Failed(String),
    /// Empty output where output was promised (section 7.2).
    Silent,
    /// The host could not be reached at all.
    Unreachable(String),
    /// The executor cannot do this on this host (no filesystem, no such
    /// probe, an unsupported primitive).
    Unsupported(String),
    Io(String),
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecError::Failed(s) => write!(f, "failed: {s}"),
            ExecError::Silent => write!(f, "silent: no output where output was promised"),
            ExecError::Unreachable(s) => write!(f, "unreachable: {s}"),
            ExecError::Unsupported(s) => write!(f, "unsupported: {s}"),
            ExecError::Io(s) => write!(f, "i/o: {s}"),
        }
    }
}

impl std::error::Error for ExecError {}

/// A host lock, released on drop (section 7.7).
pub trait HostLockGuard: Send {}

pub trait Executor: Send {
    fn locus(&self) -> LocusKind;
    fn capabilities(&self) -> ExecCaps;
    /// Run a resolved body on a host for an instance.
    fn run(&mut self, host: &Host, instance: &str, body: &[RPrim]) -> Result<Output, ExecError>;
    /// Observe a probe (by name) or a footprint fact (by shape).
    fn observe(&mut self, host: &Host, probe: &str) -> Result<Observation, ExecError>;
    /// The bytes of a file fact (`file:<path>`) as it is now; `None` when
    /// the file is absent. What a snapshot before `do` reads.
    fn read_fact(&mut self, host: &Host, shape: &str) -> Result<Option<Vec<u8>>, ExecError>;
    fn bootstrap_state(&mut self, host: &Host) -> Result<BootstrapState, ExecError>;
    // Instance directories, only where capabilities.filesystem.
    fn instance_dir_create(&mut self, host: &Host, instance: &str) -> Result<(), ExecError>;
    fn instance_dir_remove(&mut self, host: &Host, instance: &str) -> Result<(), ExecError>;
    fn instance_dir_list(&mut self, host: &Host) -> Result<Vec<InstanceDirState>, ExecError>;
    fn put_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<(), ExecError>;
    /// Write-to-temp-then-rename.
    fn replace_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
    ) -> Result<(), ExecError>;
    fn get_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<Vec<u8>, ExecError>;
    fn host_lock(&mut self, host: &Host) -> Result<Box<dyn HostLockGuard>, ExecError>;
}

// ---------------------------------------------------------------------------
// The fake: scripted outcomes, every call recorded.

/// What the next `run` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scripted {
    Ok(Output),
    Fail(String),
    Silent,
    Unreachable,
}

/// One recorded call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub host: String,
    pub instance: String,
    pub body: Vec<RPrim>,
}

/// The fake executor's state; shared with a test through [`FakeHandle`].
#[derive(Debug)]
pub struct FakeExecutor {
    pub locus: LocusKind,
    pub caps: ExecCaps,
    /// Consumed one per `run`; an empty script means every run succeeds
    /// with no output.
    pub script: VecDeque<Scripted>,
    /// Answers by probe name or fact shape; an unlisted probe is
    /// `Unsupported`.
    pub observations: BTreeMap<String, Observation>,
    pub calls: Vec<Call>,
    pub observed: Vec<(String, String)>,
    /// File facts by shape, as the target holds them; `read_fact` reads
    /// here and a `Write`/`Remove`/`RegionSet`/`RegionClear` in a run body
    /// updates it, so a restore can be checked end to end.
    pub facts: BTreeMap<String, Vec<u8>>,
    pub dirs: BTreeSet<(String, String)>,
    pub files: BTreeMap<(String, String, String), Vec<u8>>,
    pub bootstrap: BootstrapState,
}

impl FakeExecutor {
    pub fn new(locus: LocusKind) -> FakeExecutor {
        FakeExecutor {
            locus,
            caps: ExecCaps {
                filesystem: true,
                stdin_preamble: true,
            },
            script: VecDeque::new(),
            observations: BTreeMap::new(),
            calls: Vec::new(),
            observed: Vec::new(),
            facts: BTreeMap::new(),
            dirs: BTreeSet::new(),
            files: BTreeMap::new(),
            bootstrap: BootstrapState {
                rue_root: true,
                group: true,
                instances_dir: true,
                lock: true,
                modes_ok: true,
            },
        }
    }

    pub fn shared(self) -> FakeHandle {
        FakeHandle(Arc::new(Mutex::new(self)))
    }
}

/// A handle both the engine (as `Box<dyn Executor>`) and a test hold.
#[derive(Debug, Clone)]
pub struct FakeHandle(pub Arc<Mutex<FakeExecutor>>);

impl FakeHandle {
    pub fn with<T>(&self, f: impl FnOnce(&mut FakeExecutor) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn calls(&self) -> Vec<Call> {
        self.with(|f| f.calls.clone())
    }
    pub fn script(&self, outcomes: Vec<Scripted>) {
        self.with(|f| f.script.extend(outcomes));
    }
    pub fn observe_as(&self, probe: &str, o: Observation) {
        self.with(|f| {
            f.observations.insert(probe.to_string(), o);
        });
    }
}

struct FakeLock;
impl HostLockGuard for FakeLock {}

impl Executor for FakeHandle {
    fn locus(&self) -> LocusKind {
        self.with(|f| f.locus.clone())
    }
    fn capabilities(&self) -> ExecCaps {
        self.with(|f| f.caps)
    }
    fn run(&mut self, host: &Host, instance: &str, body: &[RPrim]) -> Result<Output, ExecError> {
        self.with(|f| {
            f.calls.push(Call {
                host: host.name().to_string(),
                instance: instance.to_string(),
                body: body.to_vec(),
            });
            let outcome = f.script.pop_front();
            if matches!(outcome, None | Some(Scripted::Ok(_))) {
                // A successful body acts on the fake's file facts.
                for p in body {
                    match p {
                        RPrim::Write { shape, content } => {
                            f.facts
                                .insert(shape.clone(), content.text.clone().into_bytes());
                        }
                        RPrim::Remove { shape } => {
                            f.facts.remove(shape);
                        }
                        RPrim::Append { shape, line } => {
                            let e = f.facts.entry(shape.clone()).or_default();
                            e.extend_from_slice(line.text.as_bytes());
                            e.push(b'\n');
                        }
                        RPrim::RegionSet {
                            shape,
                            anchor,
                            content,
                        } => {
                            let a = anchor.clone().unwrap_or_default();
                            let e = f.facts.entry(shape.clone()).or_default();
                            e.extend_from_slice(
                                format!(
                                    "# rue-region {a} begin\n{}\n# rue-region {a} end\n",
                                    content.text
                                )
                                .as_bytes(),
                            );
                        }
                        RPrim::RegionClear { shape, anchor } => {
                            let a = anchor.clone().unwrap_or_default();
                            if let Some(bytes) = f.facts.get_mut(shape) {
                                let text = String::from_utf8_lossy(bytes).into_owned();
                                let begin = format!("# rue-region {a} begin\n");
                                let end = format!("# rue-region {a} end\n");
                                if let (Some(s), Some(e)) = (text.find(&begin), text.find(&end)) {
                                    let mut out = text[..s].to_string();
                                    out.push_str(&text[e + end.len()..]);
                                    *bytes = out.into_bytes();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            match outcome {
                None => Ok(Output::default()),
                Some(Scripted::Ok(o)) => Ok(o),
                Some(Scripted::Fail(e)) => Err(ExecError::Failed(e)),
                Some(Scripted::Silent) => Err(ExecError::Silent),
                Some(Scripted::Unreachable) => Err(ExecError::Unreachable(host.address.clone())),
            }
        })
    }
    fn observe(&mut self, host: &Host, probe: &str) -> Result<Observation, ExecError> {
        self.with(|f| {
            f.observed
                .push((host.name().to_string(), probe.to_string()));
            f.observations
                .get(probe)
                .cloned()
                .ok_or_else(|| ExecError::Unsupported(format!("no such probe: {probe}")))
        })
    }
    fn read_fact(&mut self, _host: &Host, shape: &str) -> Result<Option<Vec<u8>>, ExecError> {
        self.with(|f| Ok(f.facts.get(shape).cloned()))
    }
    fn bootstrap_state(&mut self, _host: &Host) -> Result<BootstrapState, ExecError> {
        self.with(|f| Ok(f.bootstrap.clone()))
    }
    fn instance_dir_create(&mut self, host: &Host, instance: &str) -> Result<(), ExecError> {
        self.with(|f| {
            f.dirs
                .insert((host.name().to_string(), instance.to_string()));
            Ok(())
        })
    }
    fn instance_dir_remove(&mut self, host: &Host, instance: &str) -> Result<(), ExecError> {
        self.with(|f| {
            f.dirs
                .remove(&(host.name().to_string(), instance.to_string()));
            f.files
                .retain(|(h, i, _), _| !(h == host.name() && i == instance));
            Ok(())
        })
    }
    fn instance_dir_list(&mut self, host: &Host) -> Result<Vec<InstanceDirState>, ExecError> {
        self.with(|f| {
            Ok(f.dirs
                .iter()
                .filter(|(h, _)| h == host.name())
                .map(|(h, i)| InstanceDirState {
                    instance: i.clone(),
                    armed: f
                        .files
                        .contains_key(&(h.clone(), i.clone(), "deadline".to_string())),
                    fired: f
                        .files
                        .contains_key(&(h.clone(), i.clone(), "fired".to_string())),
                })
                .collect())
        })
    }
    fn put_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
        _mode: u32,
    ) -> Result<(), ExecError> {
        self.with(|f| {
            f.files.insert(
                (
                    host.name().to_string(),
                    instance.to_string(),
                    rel.to_string(),
                ),
                bytes.to_vec(),
            );
            Ok(())
        })
    }
    fn replace_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
    ) -> Result<(), ExecError> {
        self.put_file(host, instance, rel, bytes, 0o640)
    }
    fn get_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<Vec<u8>, ExecError> {
        self.with(|f| {
            f.files
                .get(&(
                    host.name().to_string(),
                    instance.to_string(),
                    rel.to_string(),
                ))
                .cloned()
                .ok_or_else(|| ExecError::Io(format!("no such file: {rel}")))
        })
    }
    fn host_lock(&mut self, _host: &Host) -> Result<Box<dyn HostLockGuard>, ExecError> {
        Ok(Box::new(FakeLock))
    }
}
