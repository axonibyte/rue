//! The provisioned guest: its self-check passes, and the target is reached
//! over rue's own key, through the loopback alias, with nothing read from
//! `~/.ssh`. Every later tier-5 case stands on this one.

use std::path::PathBuf;
use std::process::Command;

use rue_e2e::{e2e_root, require_provisioned_host, ssh_command, TARGET_ADDRESS, TARGET_USER};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn the_provisioning_self_check_passes() {
    require_provisioned_host();
    let out = Command::new("sh")
        .arg(repo_root().join("tenants/e2e/provision.sh"))
        .arg("check")
        .output()
        .expect("sh");
    assert!(
        out.status.success(),
        "provision.sh check failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_target_is_reached_over_rue_s_own_key_through_the_loopback_alias() {
    require_provisioned_host();
    let root = e2e_root().unwrap();
    let out = ssh_command(&root, TARGET_ADDRESS, TARGET_USER)
        .arg("printf '%s' \"$SSH_CONNECTION\"")
        .output()
        .expect("ssh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "ssh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // SSH_CONNECTION is "<client ip> <client port> <server ip> <server port>".
    let fields: Vec<&str> = stdout.split_whitespace().collect();
    assert_eq!(
        fields.get(2),
        Some(&TARGET_ADDRESS),
        "server side: {stdout}"
    );
    assert_eq!(fields.get(3), Some(&"22"), "{stdout}");
}
