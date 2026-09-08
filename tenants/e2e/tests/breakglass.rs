//! T1's shape end to end (tier 5, task 14): a plan gate two humans must
//! open, a management-controller account enabled through a hook on a host
//! with no filesystem, a secret output escrowed through another hook, and
//! a fenced block in a shared authorized-keys file on a real target over
//! a real sshd.
//!
//! The three hooks are `tenants/t1/fixtures/hooks.py`, spawned by the
//! daemon as stdio children: the authority that publishes the
//! authenticators and verifies proofs, the escrow that takes the
//! credential, and the management controller itself. They are Python
//! standard library and keep their state in a directory the harness makes.

use std::path::PathBuf;

use rue_e2e::{
    e2e_root, expect_exit, instance_of, must, python, repo_root, require_provisioned_host, rue,
    target_read, Daemon, Site,
};

/// The plan: T1's shape, with the parts a guest can carry.
const PLANS: &str = r##"
defprobe :bmc_account do
  run "account"
  locus :controller
  produces bmc.account("breakglass")
end

defop :bmc_enable, %{os: :appliance} do
  footprint modified: bmc.account("breakglass")
  do: hook(:bmc_enable, account: "breakglass")
  undo: hook(:bmc_disable, account: "breakglass", idempotent: true)
  undo_pre bmc.account("breakglass")
  undo_locus: :controller
  outputs bmc_password, secret: true
  locus: host("bmc-01")
end

defop :keys_block, _ do
  footprint region: file("/root/.ssh/authorized_keys", anchor: "rue-e2e-t1")
  do: region_set(file("/root/.ssh/authorized_keys", anchor: "rue-e2e-t1"), content: "# rue e2e t1")
  undo: :restore
  undo_locus: :target
end

defplan :breakglass, _ do
  wane 1h, renew_within: 10m
  gate thresh(2, auth(:oncall), auth(:second)), window: 30m
  bmc_enable()
  keys_block()
end
"##;

const SITE_LINES: &str = concat!(
    "  approval via: hook(:authority)\n",
    "  secrets deliver_to: [hook(:escrow)]\n",
    "  hooks do\n",
    "    registrar :host, user: :socket_owner, may_register: [:authority, :escrow, :bmc_api]\n",
    "  end\n",
);

const HOSTS: &str = concat!(
    "[[host]]\n",
    "name = \"bmc-01\"\n",
    "address = \"10.0.9.1\"\n",
    "os = \"appliance\"\n",
    "roles = [\"bmc\"]\n",
    "reach = [\"api\"]\n",
    "filesystem = false\n",
);

fn hooks_py() -> PathBuf {
    repo_root().join("tenants/t1/fixtures/hooks.py")
}

/// The digest a proof must match: the challenge the binding rendered
/// carries it, and this stub accepts the first bytes of it as the token.
fn token_from(challenge: &str) -> String {
    challenge
        .rsplit('[')
        .next()
        .unwrap_or_default()
        .trim_end_matches(']')
        .trim()
        .to_string()
}

#[test]
fn a_break_glass_plan_opens_on_two_proofs_escrows_its_secret_and_reverts() {
    require_provisioned_host();
    let state = e2e_root().unwrap().join("t1-state");
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&state).unwrap();
    // The site: T1's bindings, its inventory's second host, and the
    // management controller's transport on the one execute line.
    let site = Site::with(
        "breakglass",
        PLANS,
        SITE_LINES,
        HOSTS,
        ", hook(:bmc_api, transport: :api)",
    );
    let py = hooks_py();
    let interpreter = python();
    let spawn: Vec<String> = ["authority", "escrow", "bmc_api"]
        .iter()
        .flat_map(|h| {
            [
                "--spawn".to_string(),
                format!("{h}={interpreter} {} {h}", py.display()),
            ]
        })
        .collect();
    // The hooks keep their state where the harness can read it.
    std::env::set_var("RUE_T1_STATE", &state);
    let d = Daemon::start_with(&site, &spawn);

    // The plan is refused entry until two humans have proved themselves.
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    let line = expect_exit("apply", &out, 6);
    let id = instance_of(&line);
    assert!(line.contains("pending"), "{line}");
    assert!(!target_read("/root/.ssh/authorized_keys").contains("rue-e2e-t1"));

    // The challenge the authority renders, and the proof it accepts.
    let out = rue(&d.socket, &["approve", &id]);
    let challenge = must("approve (challenge)", &out);
    let token = token_from(&challenge);
    assert!(
        !token.is_empty(),
        "the challenge carries a digest: {challenge}"
    );

    // One proof is not two: the instance is still pending, which is exit
    // 6 and not a failure of the verb.
    let out = rue_with_stdin(
        &d.socket,
        &["approve", &id, "--authenticator", "oncall"],
        &token,
    );
    let line = expect_exit("approve (oncall)", &out, 6);
    assert!(line.contains("pending"), "one proof is not two: {line}");

    // The second opens it, and the plan runs to the end.
    let out = rue_with_stdin(
        &d.socket,
        &["approve", &id, "--authenticator", "second"],
        &token,
    );
    let line = must("approve (second)", &out);
    assert!(line.contains("applied"), "{line}");
    // The region is on the target, and the account is enabled on the
    // appliance, which has no filesystem and never had a directory.
    assert!(target_read("/root/.ssh/authorized_keys").contains("rue-e2e-t1"));
    assert_eq!(
        std::fs::read_to_string(state.join("bmc-account")).unwrap_or_default(),
        "enabled\n"
    );
    // The secret went to the escrow, by label and never by value.
    let receipts = std::fs::read_to_string(state.join("escrow-receipts")).unwrap_or_default();
    assert!(
        receipts.contains("bmc_password"),
        "the escrow took it: {receipts:?}"
    );
    let journal = std::fs::read_to_string(site.dir.join("journal.ndjson")).unwrap_or_default();
    assert!(
        !journal.contains("t1-breakglass-password"),
        "the value is in no journal"
    );
    assert!(
        journal.contains("secret_revealed"),
        "the delivery is journaled by label: {journal}"
    );

    // Recanted: the region goes and the account is disabled again.
    let out = rue(&d.socket, &["recant", &id]);
    must("recant", &out);
    assert!(!target_read("/root/.ssh/authorized_keys").contains("rue-e2e-t1"));
    assert_eq!(
        std::fs::read_to_string(state.join("bmc-account")).unwrap_or_default(),
        ""
    );
    d.stop();
}

/// `rue` with a token on its standard input.
fn rue_with_stdin(socket: &std::path::Path, args: &[&str], stdin: &str) -> std::process::Output {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(rue_e2e::bin("rue"))
        .args(args)
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rue");
    if let Some(mut w) = child.stdin.take() {
        let _ = w.write_all(stdin.as_bytes());
    }
    child.wait_with_output().expect("rue")
}
