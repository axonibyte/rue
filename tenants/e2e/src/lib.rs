//! The tier 5 and 6 harness (docs/TESTING.md, "Under reaper").
//!
//! Everything here runs on a disposable guest that `tenants/e2e/provision.sh`
//! prepared: a loopback alias the target is addressed by, sshd accepting
//! rue's own key through a drop-in `AuthorizedKeysFile`, a firewall baseline
//! that skips the management interface, the `rue` group and a `rue_root`
//! under reaper's state dataset. The material of the harness -- its key,
//! its `known_hosts` -- lives beside the working tree, never inside it and
//! never under `~/.ssh`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The address every e2e plan reaches its target by: a loopback alias, so a
/// plan that severs ssh severs only itself and never reaper's transport.
pub const TARGET_ADDRESS: &str = "127.0.0.2";
pub const TARGET_USER: &str = "root";

/// The directory holding the harness's key and `known_hosts`:
/// `RUE_E2E_ROOT`, else `rue-e2e` beside the working tree reaper names in
/// `REAPER_WORK`.
pub fn e2e_root() -> Result<PathBuf, String> {
    if let Ok(r) = env::var("RUE_E2E_ROOT") {
        if !r.is_empty() {
            return Ok(PathBuf::from(r));
        }
    }
    match env::var("REAPER_WORK") {
        Ok(w) if !w.is_empty() => {
            let work = PathBuf::from(w);
            let parent = work
                .parent()
                .ok_or_else(|| format!("REAPER_WORK {} has no parent", work.display()))?;
            Ok(parent.join("rue-e2e"))
        }
        _ => Err(
            "neither RUE_E2E_ROOT nor REAPER_WORK is set; the harness has no key material".into(),
        ),
    }
}

/// The tests run only where `run.sh` says the host is disposable and
/// provisioned. Anywhere else they refuse loudly rather than pass having
/// touched nothing.
pub fn require_provisioned_host() {
    match env::var("RUE_E2E").as_deref() {
        Ok("1") => {}
        _ => panic!(
            "tier 5 needs a provisioned disposable host: run `sh tenants/e2e/run.sh` on a reaper guest (RUE_E2E=1 is set by it, never by hand on a workstation)"
        ),
    }
}

/// An `ssh` invocation that reads nothing of the invoking user's: no
/// configuration file (`-F none`), only the named identity, only the
/// harness's `known_hosts`, and a strict host-key check against it.
pub fn ssh_command(root: &Path, address: &str, user: &str) -> Command {
    let mut c = Command::new("ssh");
    c.arg("-F")
        .arg("none")
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-i")
        .arg(root.join("keys").join("id_ed25519"))
        .arg("-o")
        .arg(format!(
            "UserKnownHostsFile={}",
            root.join("known_hosts").display()
        ))
        .arg("-o")
        .arg("GlobalKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("StrictHostKeyChecking=yes")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg(format!("{user}@{address}"));
    c
}

// ---------------------------------------------------------------------------
// The harness: a site of our own, a daemon over it, and the CLI against that.

use std::fs;
use std::io::Write;
use std::process::{Child, Stdio};

/// The repository root, from this crate's manifest.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root")
}

/// A release binary of the workspace, wherever cargo put it.
pub fn bin(name: &str) -> PathBuf {
    let target = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("target"));
    let p = target.join("release").join(name);
    assert!(
        p.exists(),
        "{} is not built; the guest's build makes the release binaries",
        p.display()
    );
    p
}

/// The `rue_root` the guest was provisioned with (7.7).
pub fn rue_root() -> PathBuf {
    if let Ok(r) = env::var("RUE_E2E_RUE_ROOT") {
        if !r.is_empty() {
            return PathBuf::from(r);
        }
    }
    let state = env::var("REAPER_STATE").expect("REAPER_STATE on a reaper guest");
    PathBuf::from(state).join("rue")
}

/// `freebsd` or `linux`: what the inventory calls this guest.
pub fn os_family() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        _ => "freebsd",
    }
}

/// The account this harness runs as, for the operators block.
pub fn me() -> String {
    for var in ["USER", "LOGNAME"] {
        if let Ok(v) = env::var(var) {
            if !v.is_empty() {
                return v;
            }
        }
    }
    "root".to_string()
}

/// The firewall file each family's plans hold a region in. Under
/// `tenants/`, where platform vocabulary belongs (section 4.4).
pub fn firewall_file() -> &'static str {
    match os_family() {
        "linux" => "/etc/nftables.conf",
        _ => "/etc/pf.conf",
    }
}

/// The command that reloads it.
pub fn firewall_reload() -> &'static str {
    match os_family() {
        "linux" => "nft -f /etc/nftables.conf",
        _ => "pfctl -f /etc/pf.conf",
    }
}

/// One scenario's site: its own directory beside the harness's key
/// material, its own inventory, its own store, and the plan text the
/// scenario applies.
pub struct Site {
    pub dir: PathBuf,
    pub file: PathBuf,
    pub store: PathBuf,
    pub socket: PathBuf,
}

impl Site {
    /// Write a site block, an inventory naming the target, and `plans`
    /// after it. The key material is the harness's, by relative path from
    /// the site file, so nothing of the invoking user's is read.
    pub fn new(name: &str, plans: &str) -> Site {
        let root = e2e_root().expect("the harness root");
        let dir = root.join(format!("case-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the case directory");
        // The key material lives one level up; the site names it by
        // relative path, as a real site would.
        fs::copy(root.join("known_hosts"), dir.join("known_hosts"))
            .expect("the harness known_hosts");
        fs::create_dir_all(dir.join("keys")).expect("keys");
        fs::copy(root.join("keys/id_ed25519"), dir.join("keys/id_ed25519"))
            .expect("the harness key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(
                dir.join("keys/id_ed25519"),
                fs::Permissions::from_mode(0o600),
            );
        }
        fs::write(
            dir.join("inventory.toml"),
            format!(
                "# The guest itself, reached through the loopback alias so a plan\n\
                 # that severs ssh severs only itself.\n\
                 [[host]]\n\
                 name = \"fw-01\"\n\
                 address = \"{TARGET_ADDRESS}\"\n\
                 os = \"{}\"\n\
                 roles = [\"fw\"]\n\
                 reach = [\"ssh\"]\n\
                 filesystem = true\n\
                 scheduler = \"cron\"\n\
                 rue_root = \"{}\"\n\
                 \n\
                 [authenticators]\n\
                 ops = {{ human = true }}\n",
                os_family(),
                rue_root().display()
            ),
        )
        .expect("the inventory");
        let file = dir.join("site.rue");
        fs::write(
            &file,
            format!(
                "rue 0\n\
                 site do\n  \
                 inventory from: file(\"inventory.toml\")\n  \
                 journal to: file(\"journal.ndjson\")\n  \
                 execute via: [local(), ssh(identity: \"keys/id_ed25519\", known_hosts: \"known_hosts\", user: \"{TARGET_USER}\")]\n  \
                 backstop scheduler: cron()\n  \
                 notify via: stdout()\n  \
                 max_wait 1h\n  \
                 operators do\n    \
                 identity :ops, user: \"{}\", operator_for: :all, admin: true\n  \
                 end\n\
                 end\n\n{plans}",
                me()
            ),
        )
        .expect("the site file");
        Site {
            store: dir.join("store"),
            socket: dir.join("rued.sock"),
            dir,
            file,
        }
    }
}

/// A running `rued` over one site.
pub struct Daemon {
    child: Child,
    pub socket: PathBuf,
}

impl Daemon {
    pub fn start(site: &Site) -> Daemon {
        // A daemon that was killed leaves its socket behind; the new one
        // removes it when it binds, and waiting for the file to appear
        // again is how the harness knows which daemon it is talking to.
        let _ = fs::remove_file(&site.socket);
        let child = Command::new(bin("rued"))
            .arg("run")
            .arg("--site")
            .arg(&site.file)
            .arg("--store")
            .arg(&site.store)
            .arg("--socket")
            .arg(&site.socket)
            .arg("--group")
            .arg("rue")
            .arg("--reap-every")
            .arg("1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("rued");
        let mut d = Daemon {
            child,
            socket: site.socket.clone(),
        };
        let start = std::time::Instant::now();
        while !d.socket.exists() {
            // A daemon that refused to start says why and stops; waiting
            // for its socket would only waste the timeout.
            if let Ok(Some(status)) = d.child.try_wait() {
                panic!("rued exited {status} before serving:\n{}", d.said());
            }
            if start.elapsed() >= std::time::Duration::from_secs(30) {
                let where_ = d.socket.display().to_string();
                panic!("rued never served {where_}:\n{}", d.said());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        d
    }

    /// What the daemon has said so far, without stopping it: for a test
    /// that has waited long enough and wants to say why.
    pub fn said(&mut self) -> String {
        let mut buf = String::new();
        if let Some(e) = self.child.stderr.as_mut() {
            use std::io::Read;
            // The pipe is drained without blocking: whatever is there.
            let mut chunk = [0u8; 8192];
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                let fd = e.as_raw_fd();
                // SAFETY: the descriptor is the child's own pipe.
                unsafe {
                    let flags = libc::fcntl(fd, libc::F_GETFL);
                    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }
            while let Ok(n) = e.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
            }
        }
        buf
    }

    /// The daemon's own process, for a scenario that kills it.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Stop it and return what it said on stderr.
    pub fn stop(mut self) -> String {
        let _ = self.child.kill();
        let mut buf = String::new();
        if let Some(mut e) = self.child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut buf);
        }
        let _ = self.child.wait();
        buf
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `rue` against a daemon's socket. The verdict line is the last line of
/// stdout, and the exit code is the outcome's (section 6.8).
pub fn rue(socket: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin("rue"))
        .args(args)
        .arg("--socket")
        .arg(socket)
        .output()
        .expect("rue")
}

/// The verdict line: the last line a verb printed. Every verb that acts
/// on the world ends in one (section 6.8), on stdout when it ran and on
/// stderr when the call itself was refused.
pub fn last_line(out: &std::process::Output) -> String {
    let pick = |b: &[u8]| {
        String::from_utf8_lossy(b)
            .lines()
            .rfind(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .to_string()
    };
    let stdout = pick(&out.stdout);
    if stdout.is_empty() {
        pick(&out.stderr)
    } else {
        stdout
    }
}

/// A verb that must have run: its verdict line, or a failure showing
/// everything both streams said, since a refusal at check is a diagnostic
/// and not a verdict.
pub fn must(what: &str, out: &std::process::Output) -> String {
    let line = last_line(out);
    assert!(
        out.status.success(),
        "{what} failed ({}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    line
}

/// The instance id a verdict line begins with.
pub fn instance_of(line: &str) -> String {
    line.split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Read a file on the target, over the harness's ssh.
pub fn target_read(path: &str) -> String {
    let root = e2e_root().expect("the harness root");
    let out = ssh_command(&root, TARGET_ADDRESS, TARGET_USER)
        .arg(format!("cat {path} 2>/dev/null || true"))
        .output()
        .expect("ssh");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Write a file on the target, behind the engine's back.
pub fn target_write(path: &str, content: &str) {
    let root = e2e_root().expect("the harness root");
    let mut child = ssh_command(&root, TARGET_ADDRESS, TARGET_USER)
        .arg(format!("cat > {path}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("ssh");
    if let Some(mut w) = child.stdin.take() {
        let _ = w.write_all(content.as_bytes());
    }
    let out = child.wait_with_output().expect("ssh");
    assert!(out.status.success(), "writing {path} on the target");
}

/// Whether a path exists on the target.
pub fn target_exists(path: &str) -> bool {
    let root = e2e_root().expect("the harness root");
    let out = ssh_command(&root, TARGET_ADDRESS, TARGET_USER)
        .arg(format!("test -e {path}"))
        .output()
        .expect("ssh");
    out.status.success()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(c: &Command) -> Vec<String> {
        c.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_ssh_command_reads_nothing_of_the_user_s_own() {
        let c = ssh_command(Path::new("/x/rue-e2e"), TARGET_ADDRESS, TARGET_USER);
        assert_eq!(c.get_program(), "ssh");
        let a = args_of(&c);
        let joined = a.join(" ");
        assert!(joined.starts_with("-F none "), "{joined}");
        assert!(joined.contains("-o IdentitiesOnly=yes -i /x/rue-e2e/keys/id_ed25519"));
        assert!(joined.contains("-o UserKnownHostsFile=/x/rue-e2e/known_hosts"));
        assert!(joined.contains("-o GlobalKnownHostsFile=/dev/null"));
        assert!(joined.contains("-o StrictHostKeyChecking=yes"));
        assert!(joined.contains("-o BatchMode=yes"));
        assert!(
            !joined.contains(".ssh"),
            "never the user's directory: {joined}"
        );
        assert_eq!(a.last().map(String::as_str), Some("root@127.0.0.2"));
    }

    #[test]
    fn the_root_is_the_declared_one_else_beside_the_reaper_work_tree_else_refused() {
        // Environment is process-wide; the three cases run in one test.
        env::set_var("RUE_E2E_ROOT", "/declared");
        env::set_var("REAPER_WORK", "/tank/work/rue");
        assert_eq!(e2e_root().unwrap(), PathBuf::from("/declared"));
        env::remove_var("RUE_E2E_ROOT");
        assert_eq!(e2e_root().unwrap(), PathBuf::from("/tank/work/rue-e2e"));
        env::remove_var("REAPER_WORK");
        assert!(e2e_root().unwrap_err().contains("no key material"));
    }
}
