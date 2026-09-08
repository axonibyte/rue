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
