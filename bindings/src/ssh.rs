//! `execute via: ssh(identity: "...", known_hosts: "...", user: "root")`:
//! a target over the system OpenSSH client (7.2, 7.4), through a
//! transport seam a test replaces with a fake.
//!
//! The client is run with `-F none`, `IdentitiesOnly=yes` and the declared
//! identity, the declared `known_hosts` as the only host-key source, and
//! `BatchMode=yes`: nothing of the invoking user's is read, ever.
//!
//! Every remote operation is one `ssh ... sh` whose script arrives on the
//! remote shell's stdin. That is the stdin preamble (7.4): a `run`'s
//! `env:` values are shell assignments decoded from octal escapes by
//! `printf '%b'` inside the script (never `SendEnv`, never `VAR=val` on a
//! command line), its `stdin:` is piped from the same decoding, and the
//! script itself is not on any command line, so the target's process list
//! shows `sh` and the plan's own commands and nothing else. The region and
//! digest helpers the script carries are the artifact's (`rue_render::
//! sh_helpers`), so a region is set and stripped by one rule here, in the
//! engine and when the artifact fires.
//!
//! The host lock is a long-lived `ssh ... lockf`/`flock` holding
//! `<rue_root>/lock` until the guard drops.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use rue_core::model::{Instant, Tri};
use rue_engine::executor::{
    BootstrapState, ExecCaps, ExecError, Executor, HostLockGuard, InstanceDirState, LocusKind,
    Observation, Output, ProbeRun, RPrim, Resolved,
};
use rue_engine::host::Host;
use rue_render::quote;

use crate::local::split_outputs;

/// What one remote script returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The seam: run a script on a host through a remote `sh`, or hold a
/// command open there.
pub trait Transport: Send {
    fn name(&self) -> String;
    /// `ssh host sh` with the script on stdin.
    fn exec(&mut self, host: &Host, script: &str) -> Result<Exit, ExecError>;
    /// Start a remote command and keep it running until the guard drops;
    /// returns once it prints `ready` on its stdout.
    fn hold(&mut self, host: &Host, script: &str) -> Result<Box<dyn HostLockGuard>, ExecError>;
}

/// The system OpenSSH client.
#[derive(Debug, Clone)]
pub struct OpenSsh {
    pub identity: PathBuf,
    pub known_hosts: PathBuf,
    pub user: String,
    pub connect_timeout: u32,
}

impl OpenSsh {
    fn command(&self, host: &Host) -> Command {
        let mut c = Command::new("ssh");
        c.arg("-F")
            .arg("none")
            .arg("-o")
            .arg("IdentitiesOnly=yes")
            .arg("-i")
            .arg(&self.identity)
            .arg("-o")
            .arg(format!("UserKnownHostsFile={}", self.known_hosts.display()))
            .arg("-o")
            .arg("GlobalKnownHostsFile=/dev/null")
            .arg("-o")
            .arg("StrictHostKeyChecking=yes")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg(format!("ConnectTimeout={}", self.connect_timeout))
            .arg(format!("{}@{}", self.user, host.address))
            .arg("sh");
        c
    }
}

impl Transport for OpenSsh {
    fn name(&self) -> String {
        "ssh".into()
    }

    fn exec(&mut self, host: &Host, script: &str) -> Result<Exit, ExecError> {
        let mut child = self
            .command(host)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ExecError::Io(format!("ssh: {e}")))?;
        if let Some(mut w) = child.stdin.take() {
            let _ = w.write_all(script.as_bytes());
        }
        let out = child
            .wait_with_output()
            .map_err(|e| ExecError::Io(e.to_string()))?;
        let code = out.status.code().unwrap_or(-1);
        // 255 is the client's own failure to connect.
        if code == 255 {
            return Err(ExecError::Unreachable(format!(
                "{}: {}",
                host.address,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(Exit {
            code,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn hold(&mut self, host: &Host, script: &str) -> Result<Box<dyn HostLockGuard>, ExecError> {
        let mut child = self
            .command(host)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ExecError::Io(format!("ssh: {e}")))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ExecError::Io("ssh stdin".into()))?;
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| ExecError::Io(e.to_string()))?;
        stdin.flush().map_err(|e| ExecError::Io(e.to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ExecError::Io("ssh stdout".into()))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| ExecError::Io(e.to_string()))?;
        if line.trim() != "ready" {
            let _ = child.kill();
            return Err(ExecError::Failed(format!(
                "the lock holder on {} said {:?}, not ready",
                host.name(),
                line.trim()
            )));
        }
        Ok(Box::new(HeldProcess {
            child,
            _stdin: stdin,
        }))
    }
}

/// A remote lock holder: dropping it closes the client's stdin, the remote
/// `cat` ends, the lock is released.
struct HeldProcess {
    child: Child,
    _stdin: std::process::ChildStdin,
}

impl HostLockGuard for HeldProcess {}

impl Drop for HeldProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// The fake transport

/// Records every script and answers from a queue (empty: exit 0, no
/// output).
#[derive(Debug, Default)]
pub struct FakeTransport {
    pub calls: Vec<(String, String)>,
    pub replies: VecDeque<Exit>,
    pub holds: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct FakeTransportHandle(pub Arc<Mutex<FakeTransport>>);

impl FakeTransportHandle {
    pub fn with<T>(&self, f: impl FnOnce(&mut FakeTransport) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn reply(&self, code: i32, stdout: &str) {
        self.with(|t| {
            t.replies.push_back(Exit {
                code,
                stdout: stdout.into(),
                stderr: String::new(),
            })
        });
    }
    pub fn scripts(&self) -> Vec<String> {
        self.with(|t| t.calls.iter().map(|(_, s)| s.clone()).collect())
    }
}

struct FakeHold;
impl HostLockGuard for FakeHold {}

impl Transport for FakeTransportHandle {
    fn name(&self) -> String {
        "fake".into()
    }
    fn exec(&mut self, host: &Host, script: &str) -> Result<Exit, ExecError> {
        self.with(|t| {
            t.calls.push((host.name().to_string(), script.to_string()));
            Ok(t.replies.pop_front().unwrap_or(Exit {
                code: 0,
                stdout: String::new(),
                stderr: String::new(),
            }))
        })
    }
    fn hold(&mut self, host: &Host, script: &str) -> Result<Box<dyn HostLockGuard>, ExecError> {
        self.with(|t| t.holds.push((host.name().to_string(), script.to_string())));
        Ok(Box::new(FakeHold))
    }
}

// ---------------------------------------------------------------------------
// Scripts

/// A value as `printf '%b'` decodes it: every byte an octal escape, so
/// nothing in it is shell syntax and no byte but NUL is unrepresentable.
pub fn octal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 5);
    for b in s.bytes() {
        let _ = write!(out, "\\0{b:03o}");
    }
    out
}

/// `$(printf '%b' '<octal>')`: the value, decoded on the target.
fn decoded(s: &str) -> String {
    format!("\"$(printf '%b' '{}')\"", octal(s))
}

/// The shell function every mode reading goes through. The two `stat`
/// dialects disagree twice over: GNU's `-c %a` is the whole mode, BSD's
/// `-f %OLp` is the permission bits without the setgid bit (which is
/// `%OMp`), and GNU reads `-f` as "the file system", which succeeds on a
/// real path and prints something else entirely. So GNU is tried first,
/// by a flag BSD refuses outright.
const MODE_FN: &str = "rue_mode() { stat -c %a \"$1\" 2>/dev/null || stat -f '%OMp%OLp' \"$1\" 2>/dev/null || echo 0; }\n";

/// A mode as the two dialects print it, without its leading zeros: BSD's
/// `%OMp%OLp` gives `0664`, GNU's `%a` gives `664`, and both mean the
/// same file.
fn octal_mode(s: &str) -> String {
    let t = s.trim().trim_start_matches('0');
    if t.is_empty() {
        "0".to_string()
    } else {
        t.to_string()
    }
}

fn q(s: &str) -> Result<String, ExecError> {
    quote::posix(s).map_err(|e| ExecError::Unsupported(format!("cannot quote for sh: {e}")))
}

fn path_of(shape: &str) -> Result<String, ExecError> {
    rue_engine::region::file_path(shape)
        .map(str::to_string)
        .ok_or_else(|| ExecError::Unsupported(format!("{shape} is not a file fact")))
}

/// The script for one run: env as decoded assignments prefixed to the
/// command, stdin piped from the decoded value.
fn run_line(cmd: &Resolved, env: &[(String, Resolved)], stdin: Option<&Resolved>) -> String {
    let mut line = String::new();
    if let Some(s) = stdin {
        let _ = write!(line, "printf '%b' '{}' | ", octal(&s.text));
    }
    for (k, v) in env {
        let _ = write!(line, "{k}={} ", decoded(&v.text));
    }
    let _ = write!(line, "sh -c {}", shell_word(&cmd.text));
    line
}

/// A word the remote sh reads back as exactly the text.
fn shell_word(s: &str) -> String {
    decoded(s)
}

fn root_of(host: &Host) -> String {
    host.rue_root.clone().unwrap_or_else(|| {
        rue_render::Instance::default_root(rue_core::artifact::shell_of(&host.record.os))
            .to_string()
    })
}

fn prelude(host: &Host, instance: Option<&str>) -> String {
    let mut s = String::from("set -e\n");
    let _ = writeln!(s, "ROOT={}", decoded(&root_of(host)));
    if let Some(i) = instance {
        let _ = writeln!(s, "INST=\"$ROOT/instances/{i}\"");
    }
    s.push_str(rue_render::sh_helpers());
    s
}

/// The lock tool of a host's OS family.
fn lock_command(host: &Host, lock: &str) -> Result<String, ExecError> {
    Ok(match host.record.os.as_str() {
        "freebsd" | "dragonfly" => format!("lockf -k -t 60 {} sh -c 'echo ready; cat'", q(lock)?),
        "linux" => format!("flock -w 60 {} sh -c 'echo ready; cat'", q(lock)?),
        other => {
            return Err(ExecError::Unsupported(format!(
                "no host lock tool is known for os {other} (lockf on FreeBSD, flock on Linux)"
            )))
        }
    })
}

/// The executor proper.
pub struct SshExecutor {
    pub transport: Box<dyn Transport>,
}

impl SshExecutor {
    pub fn new(transport: Box<dyn Transport>) -> SshExecutor {
        SshExecutor { transport }
    }

    pub fn open_ssh(identity: PathBuf, known_hosts: PathBuf, user: &str) -> SshExecutor {
        SshExecutor::new(Box::new(OpenSsh {
            identity,
            known_hosts,
            user: user.into(),
            connect_timeout: 10,
        }))
    }

    fn exec(&mut self, host: &Host, script: &str) -> Result<Exit, ExecError> {
        self.transport.exec(host, script)
    }

    fn exec_ok(&mut self, host: &Host, script: &str) -> Result<String, ExecError> {
        let x = self.exec(host, script)?;
        if x.code != 0 {
            return Err(ExecError::Failed(format!(
                "exit {}: {}",
                x.code,
                x.stderr.trim().lines().last().unwrap_or("")
            )));
        }
        Ok(x.stdout)
    }
}

impl Executor for SshExecutor {
    fn locus(&self) -> LocusKind {
        LocusKind::Ssh
    }

    fn capabilities(&self) -> ExecCaps {
        ExecCaps {
            filesystem: true,
            stdin_preamble: true,
        }
    }

    fn run(&mut self, host: &Host, instance: &str, body: &[RPrim]) -> Result<Output, ExecError> {
        let mut script = prelude(host, Some(instance));
        for p in body {
            match p {
                RPrim::Run { cmd, env, stdin } => {
                    script.push_str(&run_line(cmd, env, stdin.as_ref()));
                    script.push('\n');
                }
                RPrim::Call { cmd, .. } => {
                    script.push_str(&run_line(cmd, &[], None));
                    script.push('\n');
                }
                RPrim::Write { shape, content } => {
                    let p = q(&path_of(shape)?)?;
                    let _ = writeln!(
                        script,
                        "printf '%b' '{}' > {p}.rue-tmp && mv {p}.rue-tmp {p}",
                        octal(&content.text)
                    );
                }
                RPrim::Remove { shape } => {
                    let _ = writeln!(script, "rm -f {}", q(&path_of(shape)?)?);
                }
                RPrim::Append { shape, line } => {
                    let _ = writeln!(
                        script,
                        "printf '%b\\n' '{}' >> {}",
                        octal(&line.text),
                        q(&path_of(shape)?)?
                    );
                }
                RPrim::RegionSet {
                    shape,
                    anchor,
                    content,
                } => {
                    let _ = writeln!(
                        script,
                        "region_set {} {} {}",
                        q(&path_of(shape)?)?,
                        q(anchor.as_deref().unwrap_or(""))?,
                        decoded(&content.text)
                    );
                }
                RPrim::RegionClear { shape, anchor } => {
                    let _ = writeln!(
                        script,
                        "strip_region {} {}",
                        q(&path_of(shape)?)?,
                        q(anchor.as_deref().unwrap_or(""))?
                    );
                }
                RPrim::Stage {
                    name,
                    content,
                    mode,
                } => {
                    let n = q(name)?;
                    let _ = writeln!(
                        script,
                        "mkdir -p \"$INST\" && printf '%b' '{}' > \"$INST\"/{n}.rue-tmp && mv \"$INST\"/{n}.rue-tmp \"$INST\"/{n} && chmod {mode:o} \"$INST\"/{n}",
                        octal(&content.text)
                    );
                }
                RPrim::Hook { name, .. } => {
                    return Err(ExecError::Unsupported(format!(
                        "hook(:{name}) is a hook executor's primitive, not ssh()'s"
                    )))
                }
                RPrim::Install { name } | RPrim::Release { name } => {
                    return Err(ExecError::Unsupported(format!(
                        "{name}: install and release are a hook's primitives, not ssh()'s"
                    )))
                }
            }
        }
        let stdout = self.exec_ok(host, &script)?;
        let (rest, outputs) = split_outputs(&stdout);
        Ok(Output {
            stdout: rest,
            outputs,
        })
    }

    fn observe(&mut self, host: &Host, probe: &ProbeRun) -> Result<Observation, ExecError> {
        let run = probe.body.iter().find_map(|p| match p {
            RPrim::Run { cmd, env, stdin } => Some((cmd, env.as_slice(), stdin.as_ref())),
            _ => None,
        });
        let (cmd, env, stdin) = run.ok_or_else(|| {
            ExecError::Unsupported(format!(
                "probe {} has no run body; ssh() knows no probe by name",
                probe.name
            ))
        })?;
        let mut script = prelude(host, None);
        // The probe's own status is the answer: no `set -e` abort.
        script = script.replacen("set -e\n", "", 1);
        script.push_str(&run_line(cmd, env, stdin));
        script.push('\n');
        let x = self.exec(host, &script)?;
        let tri = match x.code {
            0 => Tri::Yes,
            1 => Tri::No,
            _ => Tri::Unknown,
        };
        Ok(Observation {
            text: x.stdout.trim_end().to_string(),
            tri: Some(tri),
        })
    }

    fn read_fact(&mut self, host: &Host, shape: &str) -> Result<Option<Vec<u8>>, ExecError> {
        let p = q(&path_of(shape)?)?;
        let script = format!("if [ -f {p} ]; then cat {p}; else exit 3; fi\n");
        let x = self.exec(host, &script)?;
        match x.code {
            0 => Ok(Some(x.stdout.into_bytes())),
            3 => Ok(None),
            c => Err(ExecError::Failed(format!("exit {c}: {}", x.stderr.trim()))),
        }
    }

    /// The target's clock, in one word on stdout: what an arm compares
    /// with the controller's before it writes a deadline (R0403).
    fn clock_now(&mut self, host: &Host) -> Result<Option<Instant>, ExecError> {
        let out = self.exec_ok(host, "date +%s\n")?;
        match out.trim().parse::<u64>() {
            Ok(s) => Ok(Some(Instant::new(s))),
            Err(_) => Err(ExecError::Failed(format!(
                "{}: the clock probe answered {:?}, not an epoch second",
                host.name(),
                out.trim()
            ))),
        }
    }

    fn bootstrap_state(&mut self, host: &Host) -> Result<BootstrapState, ExecError> {
        let script = format!(
            "{}\
             r=0; [ -d \"$ROOT\" ] && r=1\n\
             g=0; if getent group rue >/dev/null 2>&1 || pw groupshow rue >/dev/null 2>&1; then g=1; fi\n\
             i=0; [ -d \"$ROOT/instances\" ] && i=1\n\
             l=0; [ -f \"$ROOT/lock\" ] && l=1\n\
             {MODE_FN}\
             mi=$(rue_mode \"$ROOT/instances\")\n\
             ml=$(rue_mode \"$ROOT/lock\")\n\
             echo \"root=$r group=$g instances=$i lock=$l mi=$mi ml=$ml\"\n",
            prelude(host, None).replacen("set -e\n", "", 1)
        );
        let out = self.exec_ok(host, &script)?;
        let field = |k: &str| {
            out.split_whitespace()
                .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                .unwrap_or("0")
                .to_string()
        };
        Ok(BootstrapState {
            rue_root: field("root") == "1",
            group: field("group") == "1",
            instances_dir: field("instances") == "1",
            lock: field("lock") == "1",
            // BSD's `stat -f %OLp` is the permission bits alone and the
            // setgid bit is `%OMp`, so the two are read together; GNU's
            // `%a` is the whole mode already. Either way a leading zero
            // is a spelling, not a difference.
            modes_ok: octal_mode(&field("mi")) == "2770" && octal_mode(&field("ml")) == "664",
        })
    }

    fn instance_dir_create(&mut self, host: &Host, instance: &str) -> Result<(), ExecError> {
        let script = format!(
            "{}mkdir -p \"$INST/markers\" \"$INST/snapshots\" && chmod 2770 \"$INST\"\n",
            prelude(host, Some(instance))
        );
        self.exec_ok(host, &script).map(|_| ())
    }

    fn instance_dir_remove(&mut self, host: &Host, instance: &str) -> Result<(), ExecError> {
        let script = format!("{}rm -rf \"$INST\"\n", prelude(host, Some(instance)));
        self.exec_ok(host, &script).map(|_| ())
    }

    fn instance_dir_list(&mut self, host: &Host) -> Result<Vec<InstanceDirState>, ExecError> {
        let script = format!(
            "{}{MODE_FN}for d in \"$ROOT\"/instances/*/; do [ -d \"$d\" ] || continue; n=$(basename \"$d\"); a=0; f=0; [ -f \"$d/fired\" ] && f=1; for x in artifact.sh artifact.ps1 artifact.py; do [ -f \"$d/$x\" ] && [ \"$f\" -eq 0 ] && a=1; done; m=$(rue_mode \"$d\"); echo \"$n $a $f $m\"; done\n",
            prelude(host, None)
        );
        let out = self.exec_ok(host, &script)?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                let n = it.next()?;
                let a = it.next()? == "1";
                let f = it.next()? == "1";
                let m = octal_mode(it.next().unwrap_or("0"));
                Some(InstanceDirState {
                    instance: n.to_string(),
                    armed: a,
                    fired: f,
                    modes_ok: m == "2770",
                })
            })
            .collect())
    }

    fn put_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
        mode: u32,
    ) -> Result<(), ExecError> {
        let r = q(rel)?;
        let script = format!(
            "{}mkdir -p \"$(dirname \"$INST\"/{r})\" && printf '%b' '{}' > \"$INST\"/{r}.rue-tmp && chmod {mode:o} \"$INST\"/{r}.rue-tmp && mv \"$INST\"/{r}.rue-tmp \"$INST\"/{r}\n",
            prelude(host, Some(instance)),
            octal(&String::from_utf8_lossy(bytes))
        );
        self.exec_ok(host, &script).map(|_| ())
    }

    fn replace_file(
        &mut self,
        host: &Host,
        instance: &str,
        rel: &str,
        bytes: &[u8],
    ) -> Result<(), ExecError> {
        let r = q(rel)?;
        let script = format!(
            "{}mkdir -p \"$(dirname \"$INST\"/{r})\" && printf '%b' '{}' > \"$INST\"/{r}.rue-tmp && mv \"$INST\"/{r}.rue-tmp \"$INST\"/{r}\n",
            prelude(host, Some(instance)),
            octal(&String::from_utf8_lossy(bytes))
        );
        self.exec_ok(host, &script).map(|_| ())
    }

    fn get_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<Vec<u8>, ExecError> {
        let r = q(rel)?;
        let script = format!("{}cat \"$INST\"/{r}\n", prelude(host, Some(instance)));
        self.exec_ok(host, &script).map(String::into_bytes)
    }

    fn remove_file(&mut self, host: &Host, instance: &str, rel: &str) -> Result<(), ExecError> {
        let r = q(rel)?;
        let script = format!("{}rm -f \"$INST\"/{r}\n", prelude(host, Some(instance)));
        self.exec_ok(host, &script).map(|_| ())
    }

    fn host_lock(&mut self, host: &Host) -> Result<Box<dyn HostLockGuard>, ExecError> {
        let lock = format!("{}/lock", root_of(host));
        let script = format!("{}\n", lock_command(host, &lock)?);
        self.transport.hold(host, &script)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_escapes_every_byte_and_the_run_line_never_carries_a_value_bare() {
        assert_eq!(octal("a\n'"), "\\0141\\0012\\0047");
        let cmd = Resolved::plain("echo hi");
        let env = vec![(
            "TOKEN".to_string(),
            Resolved {
                text: "s3cr3t' $(x)".into(),
                secret: true,
            },
        )];
        let stdin = Resolved::plain("in\nput");
        let line = run_line(&cmd, &env, Some(&stdin));
        assert!(!line.contains("s3cr3t"), "{line}");
        assert!(!line.contains("echo hi"), "{line}");
        assert!(line.starts_with("printf '%b' '"), "{line}");
        assert!(line.contains("TOKEN=\"$(printf '%b' '"), "{line}");
        assert!(line.ends_with("')\""), "{line}");
    }
}
