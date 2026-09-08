//! `execute via: local()`: the controller machine as a target (7.2, 7.3).
//!
//! A `run` is `sh -c` with `env:` set on the child process directly and
//! `stdin:` fed on its stdin: never on a command line, never in the
//! process list. Declared outputs are read back from stdout lines of the
//! form `rue-output NAME=VALUE` (the convention docs/LANGUAGE.md states);
//! the rest of stdout is the run's text. A probe is its first `run`,
//! answering by exit status (0 yes, 1 no, else unknown), its stdout the
//! fact. File primitives act in process, regions by `engine::region`'s
//! rule; every write goes through a temporary name and a rename.
//!
//! Secrets: a value flagged secret is never placed on argv; stdout and
//! stderr that would be reported are scrubbed of every secret value the
//! body carried, so a command that echoes one cannot put it in a journal.
//! The instance directory is the host's `rue_root` (`/var/db/rue` on this
//! family) and the host lock is `flock` on `<rue_root>/lock`.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rue_core::model::Tri;
use rue_engine::executor::{
    BootstrapState, ExecCaps, ExecError, Executor, HostLockGuard, InstanceDirState, LocusKind,
    Observation, Output, ProbeRun, RPrim, Resolved,
};
use rue_engine::host::Host;
use rue_engine::region;

/// The stdout line that binds a declared output.
pub const OUTPUT_PREFIX: &str = "rue-output ";

#[derive(Debug, Clone)]
pub struct LocalExecutor {
    /// The shell every `run` goes through.
    pub shell: PathBuf,
    /// `rue_root` for a host that declares none.
    pub default_root: PathBuf,
}

impl Default for LocalExecutor {
    fn default() -> LocalExecutor {
        LocalExecutor {
            shell: PathBuf::from("/bin/sh"),
            default_root: PathBuf::from(rue_render::Instance::default_root(
                rue_core::artifact::Shell::Posix,
            )),
        }
    }
}

impl LocalExecutor {
    fn root(&self, host: &Host) -> PathBuf {
        host.rue_root
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_root.clone())
    }

    fn instance_dir(&self, host: &Host, instance: &str) -> PathBuf {
        self.root(host).join("instances").join(instance)
    }

    /// Run one shell command with its environment and stdin; the result
    /// scrubbed of every secret.
    fn sh(
        &self,
        cmd: &Resolved,
        env: &[(String, Resolved)],
        stdin: Option<&Resolved>,
        secrets: &[String],
    ) -> Result<(i32, String, String), ExecError> {
        let mut c = Command::new(&self.shell);
        c.arg("-c").arg(&cmd.text);
        for (k, v) in env {
            c.env(k, &v.text);
        }
        c.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| ExecError::Io(e.to_string()))?;
        if let Some(s) = stdin {
            if let Some(mut w) = child.stdin.take() {
                let _ = w.write_all(s.text.as_bytes());
            }
        }
        let out = child
            .wait_with_output()
            .map_err(|e| ExecError::Io(e.to_string()))?;
        let code = out.status.code().unwrap_or(-1);
        Ok((
            code,
            scrub(&String::from_utf8_lossy(&out.stdout), secrets),
            scrub(&String::from_utf8_lossy(&out.stderr), secrets),
        ))
    }
}

/// Every secret value the body carries, for scrubbing.
fn secrets_of(body: &[RPrim]) -> Vec<String> {
    let mut v = Vec::new();
    let mut push = |r: &Resolved| {
        if r.secret && !r.text.is_empty() {
            v.push(r.text.clone());
        }
    };
    for p in body {
        match p {
            RPrim::Run { cmd, env, stdin } => {
                push(cmd);
                env.iter().for_each(|(_, r)| push(r));
                if let Some(s) = stdin {
                    push(s);
                }
            }
            RPrim::Write { content, .. }
            | RPrim::RegionSet { content, .. }
            | RPrim::Stage { content, .. } => push(content),
            RPrim::Append { line, .. } => push(line),
            RPrim::Hook { args, .. } => args.iter().for_each(|(_, r)| push(r)),
            RPrim::Call { cmd, .. } => push(cmd),
            RPrim::Remove { .. }
            | RPrim::RegionClear { .. }
            | RPrim::Install { .. }
            | RPrim::Release { .. } => {}
        }
    }
    v
}

fn scrub(text: &str, secrets: &[String]) -> String {
    let mut t = text.to_string();
    for s in secrets {
        t = t.replace(s, "<secret>");
    }
    t
}

/// Write through a temporary name beside the file and a rename, keeping an
/// existing file's mode.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(match path.extension() {
        Some(e) => format!("{}.rue-tmp", e.to_string_lossy()),
        None => "rue-tmp".to_string(),
    });
    let mode = fs::metadata(path).ok().map(|m| m.permissions());
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Some(p) = mode {
        fs::set_permissions(&tmp, p)?;
    }
    fs::rename(&tmp, path)
}

fn path_of(shape: &str) -> Result<PathBuf, ExecError> {
    region::file_path(shape)
        .map(PathBuf::from)
        .ok_or_else(|| ExecError::Unsupported(format!("{shape} is not a file fact")))
}

fn read_text(path: &Path) -> std::io::Result<String> {
    match fs::read(path) {
        Ok(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

fn io(e: std::io::Error) -> ExecError {
    ExecError::Io(e.to_string())
}

/// The outputs a run's stdout declares, and the stdout without those lines.
pub fn split_outputs(stdout: &str) -> (String, std::collections::BTreeMap<String, String>) {
    let mut outputs = std::collections::BTreeMap::new();
    let mut rest = String::new();
    for line in stdout.lines() {
        match line.strip_prefix(OUTPUT_PREFIX) {
            Some(kv) => {
                if let Some((k, v)) = kv.split_once('=') {
                    outputs.insert(k.trim().to_string(), v.to_string());
                }
            }
            None => {
                rest.push_str(line);
                rest.push('\n');
            }
        }
    }
    (rest, outputs)
}

struct FlockGuard(#[allow(dead_code)] File);
impl HostLockGuard for FlockGuard {}

#[cfg(unix)]
fn flock(f: &File) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: flock on a descriptor this File owns.
    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn flock(_f: &File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn mode_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .ok()
        .map(|m| m.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_of(_path: &Path) -> Option<u32> {
    None
}

impl Executor for LocalExecutor {
    fn locus(&self) -> LocusKind {
        LocusKind::Local
    }

    fn capabilities(&self) -> ExecCaps {
        ExecCaps {
            filesystem: true,
            stdin_preamble: true,
        }
    }

    fn run(&mut self, host: &Host, instance: &str, body: &[RPrim]) -> Result<Output, ExecError> {
        let secrets = secrets_of(body);
        let mut out = Output::default();
        for p in body {
            match p {
                RPrim::Run { .. } | RPrim::Call { .. } => {
                    let (cmd, env, stdin): (&Resolved, &[(String, Resolved)], Option<&Resolved>) =
                        match p {
                            RPrim::Run { cmd, env, stdin } => (cmd, env, stdin.as_ref()),
                            RPrim::Call { cmd, .. } => (cmd, &[], None),
                            _ => unreachable!(),
                        };
                    let (code, stdout, stderr) = self.sh(cmd, env, stdin, &secrets)?;
                    if code != 0 {
                        return Err(ExecError::Failed(format!(
                            "exit {code}: {}",
                            stderr.trim().lines().last().unwrap_or("")
                        )));
                    }
                    let (rest, outputs) = split_outputs(&stdout);
                    out.stdout.push_str(&rest);
                    out.outputs.extend(outputs);
                }
                RPrim::Write { shape, content } => {
                    let path = path_of(shape)?;
                    write_atomic(&path, content.text.as_bytes()).map_err(io)?;
                }
                RPrim::Remove { shape } => {
                    let path = path_of(shape)?;
                    match fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(io(e)),
                    }
                }
                RPrim::Append { shape, line } => {
                    let path = path_of(shape)?;
                    let mut text = read_text(&path).map_err(io)?;
                    if !text.is_empty() && !text.ends_with('\n') {
                        text.push('\n');
                    }
                    text.push_str(&line.text);
                    text.push('\n');
                    write_atomic(&path, text.as_bytes()).map_err(io)?;
                }
                RPrim::RegionSet {
                    shape,
                    anchor,
                    content,
                } => {
                    let path = path_of(shape)?;
                    let a = anchor.clone().unwrap_or_default();
                    let text = read_text(&path).map_err(io)?;
                    write_atomic(&path, region::set(&text, &a, &content.text).as_bytes())
                        .map_err(io)?;
                }
                RPrim::RegionClear { shape, anchor } => {
                    let path = path_of(shape)?;
                    let a = anchor.clone().unwrap_or_default();
                    let text = read_text(&path).map_err(io)?;
                    match region::strip(&text, &a) {
                        Some(t) => write_atomic(&path, t.as_bytes()).map_err(io)?,
                        None => {
                            return Err(ExecError::Failed(format!(
                                "region {a} of {} has damaged markers",
                                path.display()
                            )))
                        }
                    }
                }
                RPrim::Stage {
                    name,
                    content,
                    mode,
                } => {
                    let dir = self.instance_dir(host, instance);
                    fs::create_dir_all(&dir).map_err(io)?;
                    let p = dir.join(name);
                    write_atomic(&p, content.text.as_bytes()).map_err(io)?;
                    set_mode(&p, *mode).map_err(io)?;
                }
                RPrim::Hook { name, .. } => {
                    return Err(ExecError::Unsupported(format!(
                        "hook(:{name}) is a hook executor's primitive, not local()'s"
                    )))
                }
                RPrim::Install { name } | RPrim::Release { name } => {
                    return Err(ExecError::Unsupported(format!(
                        "{name}: install and release are a hook's primitives, not local()'s"
                    )))
                }
            }
        }
        Ok(out)
    }

    fn observe(&mut self, _host: &Host, probe: &ProbeRun) -> Result<Observation, ExecError> {
        let run = probe.body.iter().find_map(|p| match p {
            RPrim::Run { cmd, env, stdin } => Some((cmd, env.as_slice(), stdin.as_ref())),
            _ => None,
        });
        let (cmd, env, stdin) = run.ok_or_else(|| {
            ExecError::Unsupported(format!(
                "probe {} has no run body; local() knows no probe by name",
                probe.name
            ))
        })?;
        let (code, stdout, _stderr) = self.sh(cmd, env, stdin, &secrets_of(&probe.body))?;
        let tri = match code {
            0 => Tri::Yes,
            1 => Tri::No,
            _ => Tri::Unknown,
        };
        Ok(Observation {
            text: stdout.trim_end().to_string(),
            tri: Some(tri),
        })
    }

    fn read_fact(&mut self, _host: &Host, shape: &str) -> Result<Option<Vec<u8>>, ExecError> {
        let path = path_of(shape)?;
        match fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(e)),
        }
    }

    fn bootstrap_state(&mut self, host: &Host) -> Result<BootstrapState, ExecError> {
        let root = self.root(host);
        let group = group_exists("rue");
        let instances = root.join("instances");
        let lock = root.join("lock");
        let modes_ok = mode_of(&instances) == Some(0o2770) && mode_of(&lock) == Some(0o664);
        Ok(BootstrapState {
            rue_root: root.is_dir(),
            group,
            instances_dir: instances.is_dir(),
            lock: lock.is_file(),
            modes_ok,
        })
    }

    fn instance_dir_create(&mut self, host: &Host, instance: &str) -> Result<(), ExecError> {
        let dir = self.instance_dir(host, instance);
        fs::create_dir_all(dir.join("markers")).map_err(io)?;
        fs::create_dir_all(dir.join("snapshots")).map_err(io)?;
        set_mode(&dir, 0o2770).map_err(io)?;
        Ok(())
    }

    fn instance_dir_remove(&mut self, host: &Host, instance: &str) -> Result<(), ExecError> {
        let dir = self.instance_dir(host, instance);
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
        }
    }

    fn instance_dir_list(&mut self, host: &Host) -> Result<Vec<InstanceDirState>, ExecError> {
        let dir = self.root(host).join("instances");
        let mut v = Vec::new();
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(v),
            Err(e) => return Err(io(e)),
        };
        for e in entries {
            let e = e.map_err(io)?;
            if !e.path().is_dir() {
                continue;
            }
            let p = e.path();
            v.push(InstanceDirState {
                instance: e.file_name().to_string_lossy().into_owned(),
                armed: p.join("deadline").exists() && !p.join("fired").exists(),
                fired: p.join("fired").exists(),
            });
        }
        v.sort_by(|a, b| a.instance.cmp(&b.instance));
        Ok(v)
    }

    fn put_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<(), ExecError> {
        let p = self.instance_dir(host, instance).join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        write_atomic(&p, bytes).map_err(io)?;
        set_mode(&p, mode).map_err(io)
    }

    fn replace_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
    ) -> Result<(), ExecError> {
        let p = self.instance_dir(host, instance).join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        write_atomic(&p, bytes).map_err(io)
    }

    fn get_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<Vec<u8>, ExecError> {
        fs::read(self.instance_dir(host, instance).join(rel)).map_err(io)
    }

    fn remove_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<(), ExecError> {
        match fs::remove_file(self.instance_dir(host, instance).join(rel)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
        }
    }

    fn host_lock(&mut self, host: &Host) -> Result<Box<dyn HostLockGuard>, ExecError> {
        let path = self.root(host).join("lock");
        let f = OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|e| ExecError::Io(format!("{}: {e}", path.display())))?;
        flock(&f).map_err(io)?;
        Ok(Box::new(FlockGuard(f)))
    }
}

#[cfg(unix)]
fn group_exists(name: &str) -> bool {
    let c = match std::ffi::CString::new(name) {
        Ok(c) => c,
        Err(_) => return false,
    };
    // SAFETY: getgrnam reads a NUL-terminated name.
    !unsafe { libc::getgrnam(c.as_ptr()) }.is_null()
}

#[cfg(not(unix))]
fn group_exists(_name: &str) -> bool {
    false
}

#[allow(dead_code)]
fn _read_all(mut r: impl Read) -> String {
    let mut s = String::new();
    let _ = r.read_to_string(&mut s);
    s
}
