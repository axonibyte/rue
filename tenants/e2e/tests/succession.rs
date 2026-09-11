//! T2 end to end (docs/ROADMAP.md 8.2): cluster succession, on the FreeBSD
//! guest, with the cluster made real wherever a disposable machine can make
//! it real.
//!
//! node-b is the guest itself, over ssh, and the guests it starts are
//! jails -- real kernel objects, observed with jls. The corpse, node-a, is
//! never reached: everything done to it is done through the cluster driver
//! (tenants/t2/fixtures/cluster.py), exactly as tenants/t2/plan.rue reaches
//! it. node-c is on the console, so the heir's step defers and a person
//! continues it. The rollback knell acts on a real ZFS dataset with an
//! @split snapshot, and its cost is the real list of what `zfs rollback -r`
//! will destroy.
//!
//! FreeBSD only, because jail(8) is: run.sh withholds this stage elsewhere
//! and says why.
//!
//! What this proves that no unit test can: T2 RUNS. For four phases its text
//! checked clean and could not have executed a step past its probes --
//! every guard named nothing, and the controller could perform none of the
//! cluster's actions.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use rue_e2e::{
    e2e_root, expect_exit, instance_of, me, must, os_family, python, repo_root,
    require_provisioned_host, rue, rue_root, rue_with_stdin, token_from, Daemon, Site,
    TARGET_ADDRESS,
};

const INVENTORY: &str = r#"# T2's cluster as the guest can hold it: node-b is the guest, node-c is
# reachable only from its console, and node-a is not a host at all -- only
# the cluster driver ever touches it.
[[host]]
name = "node-b"
address = "@ADDR@"
os = "@OS@"
roles = ["hv"]
reach = ["ssh"]
filesystem = true
scheduler = "cron"
rue_root = "@ROOT@"

[[host]]
name = "node-c"
address = "10.0.7.13"
os = "freebsd"
roles = ["hv"]
reach = ["console"]
filesystem = true

[authenticators]
operator = { human = true }
second_operator = { human = true }
fence_driver = { human = false }
"#;

const TEXT: &str = r#"rue 0
site do
  inventory from: file("inventory.toml")
  journal to: file("journal.ndjson")
  approval via: hook(:authority)
  execute via: [ssh(identity: "keys/id_ed25519", known_hosts: "known_hosts", user: "root"), hook(:cluster, transport: :controller)]
  backstop scheduler: cron()
  max_wait 30m
  operators do
    identity :ops, user: "@ME@", operator_for: :all, admin: true
  end
  hooks do
    registrar :host, user: :socket_owner, may_register: [:cluster, :authority]
  end
end

defprobe :peer_dead do
  hook :cluster
  locus :controller
end

defprobe :probes_agree do
  hook :cluster
  locus :controller
end

defprobe :fence_verified_off do
  hook :cluster
  locus :controller
end

defprobe :fence_verdict do
  hook :cluster
  locus :controller
end

defprobe :heir_running_on_c do
  hook :cluster
  locus :controller
end

defprobe :written_bytes_since_split do
  hook :cluster
  locus :controller
  static true
end

defprobe :datasets_ahead do
  run "zfs list -H -t snapshot -o name -r @DS@ | grep -v '@split$' | grep -q ."
  locus :target
end

defprobe :destroyed_snapshots do
  run "zfs list -H -t snapshot -o name -r @DS@ | grep -v '@split$'"
  locus :target
end

defop :fence_corpse, _, ack: ack do
  footprint
  do: hook(:fence, node: corpse)
  refusal: knell, guard: fence_verified_off, cost: fence_verdict(), ack: ack
  undo_locus: :none
  locus: :controller
end

defop :rollback_ahead_datasets, _ do
  footprint
  do: run("zfs rollback -r @DS@@split")
  refusal: knell, guard: datasets_ahead, cost: destroyed_snapshots(), ack: humans()
  undo_locus: :none
end

defop :resurrection_gate, _ do
  footprint modified: platform.mode("node-a")
  do: hook(:platform, set: :slave, node: "node-a")
  undo: hook(:platform, set: :master, node: "node-a", idempotent: true)
  undo_pre platform.mode("node-a")
  refusal: :hold, via: :slave_mode
  undo_locus: :controller
  locus: :controller
end

defprobe :guest_state do
  run "jls -j rue-t2-#{g} jid"
  reads guest.state(g)
end

defop :start_guest, %{os: :freebsd} do
  footprint modified: guest.state(g)
  do: run("jail -c name=rue-t2-#{g} persist")
  undo: run("jail -r rue-t2-#{g}", idempotent: true)
  undo_pre guest.state(g)
  refusal: :hold
  undo_locus: :controller
end

defop :record_succession, _ do
  footprint append_only: file("@LOG@"), append_only: record.placement
  do: [append(file("@LOG@"), line: entry), hook(:placement, set: entry)]
  undo: compensate: [append(file("@LOG@"), line: reversal), hook(:placement, set: reversal)]
  undo_pre file("@LOG@"), record.placement
  undo_locus: :controller
  locus: :controller
end

defop :start_heir, %{os: :freebsd} do
  footprint modified: guest.state("heir")
  do: run("jail -c name=rue-t2-heir persist")
  undo: run("jail -r rue-t2-heir", idempotent: true)
  undo_pre guest.state("heir")
  refusal: :hold
  undo_locus: :controller
  locus: host("node-c")
  handoff_done: heir_running_on_c()
end

defplan :promote_auto, %{name: "node-b"} do
  mode: :auto
  exclusivity: :"corpse:node-a"
  preflight do
    written_bytes_since_split
  end
  assert peer_dead
  assert force: never, probes_agree
  knell fence_corpse(ack: :none, reason: "the driver's verified-off is the automation's own evidence")
  resurrection_gate()
  repeat over: guests, as g, max: 16 do
    start_guest(g: g)
  end
  record_succession(entry: succession_entry)
  start_heir()
  commit()
end

defplan :promote, %{name: "node-b"} do
  exclusivity: :"corpse:node-a"
  preflight do
    written_bytes_since_split
  end
  assert peer_dead
  assert force: never, probes_agree
  knell fence_corpse(ack: humans())
  when datasets_ahead do
    knell rollback_ahead_datasets()
  end
  resurrection_gate()
  repeat over: guests, as g, max: 16 do
    start_guest(g: g)
  end
  record_succession(entry: succession_entry)
  start_heir()
  commit()
end
"#;

// --- the cluster, as the harness holds it ----------------------------------

/// A running site, its daemon, the cluster driver's state directory and the
/// dataset the rollback knell acts on.
struct Cluster {
    site: Site,
    d: Daemon,
    state: PathBuf,
    dataset: String,
}

impl Cluster {
    fn start(name: &str) -> Cluster {
        require_provisioned_host();
        release_jails();
        let root = e2e_root().expect("the harness root");
        let dataset = std::fs::read_to_string(root.join("t2-dataset"))
            .expect("provision.sh records T2's dataset")
            .trim()
            .to_string();
        let state = root.join(format!("t2-state-{name}"));
        let _ = std::fs::remove_dir_all(&state);
        std::fs::create_dir_all(&state).unwrap();
        let log = state.join("succession.log");
        let text = TEXT
            .replace("@ME@", &me())
            .replace("@DS@", &dataset)
            .replace("@LOG@", log.to_str().unwrap());
        let inventory = INVENTORY
            .replace("@ADDR@", TARGET_ADDRESS)
            .replace("@OS@", os_family())
            .replace("@ROOT@", rue_root().to_str().unwrap());
        let site = Site::raw(name, &text, &inventory);
        // The spawned children inherit the daemon's environment, and the
        // daemon inherits this process's.
        std::env::set_var("RUE_T2_STATE", &state);
        let py = python();
        let fixture = repo_root().join("tenants/t2/fixtures/cluster.py");
        let spawn: Vec<String> = ["cluster", "authority"]
            .iter()
            .flat_map(|h| {
                [
                    "--spawn".to_string(),
                    format!("{h}={py} {} {h}", fixture.display()),
                ]
            })
            .collect();
        let d = Daemon::start_with(&site, &spawn);
        Cluster {
            site,
            d,
            state,
            dataset,
        }
    }

    /// A knob the cluster driver reads: `fence`, `written`, `heir`.
    fn set(&self, knob: &str, value: &str) {
        std::fs::write(self.state.join(knob), format!("{value}\n")).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.state.join(name)).unwrap_or_default()
    }

    fn journal(&self) -> String {
        std::fs::read_to_string(self.site.dir.join("journal.ndjson")).unwrap_or_default()
    }

    fn promote(&self, plan: &str, entry: &str) -> std::process::Output {
        rue(
            &self.d.socket,
            &[
                "apply",
                self.site.file.to_str().unwrap(),
                "--host",
                "node-b",
                "--plan-name",
                plan,
                "--set",
                "corpse=node-a",
                "--set",
                "guests=g1,g2",
                "--set",
                &format!("succession_entry={entry}"),
                "--set",
                &format!("reversal=reversed: {entry}"),
            ],
        )
    }

    /// Acknowledge a knell as a human would: ask for the challenge, prove
    /// it, and submit the proof with a reason.
    fn acknowledge(&self, id: &str, step: u32, reason: &str) -> std::process::Output {
        let step = step.to_string();
        let args = [
            "ack",
            id,
            "--step",
            step.as_str(),
            "--reason",
            reason,
            "--authenticator",
            "operator",
        ];
        let challenge = rue_with_stdin(&self.d.socket, &args, "");
        let challenge = must("the acknowledgement's challenge", &challenge);
        let token = token_from(&challenge);
        assert!(
            !token.is_empty(),
            "the challenge carries a digest: {challenge}"
        );
        rue_with_stdin(&self.d.socket, &args, &token)
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        release_jails();
    }
}

/// The jails the plan's guests are, by name.
fn jails() -> Vec<String> {
    let out = Command::new("jls")
        .args(["-N", "name"])
        .output()
        .expect("jls");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| l.starts_with("rue-t2-"))
        .collect()
}

/// Remove every jail a T2 plan started: a committed promote leaves its
/// guests running, which is the point of committing, and the next case
/// needs a clean slate.
fn release_jails() {
    for j in jails() {
        let _ = Command::new("jail").args(["-r", &j]).status();
    }
}

fn zfs(args: &[&str]) -> String {
    let out = Command::new("zfs").args(args).output().expect("zfs");
    assert!(
        out.status.success(),
        "zfs {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn snapshots(dataset: &str) -> Vec<String> {
    zfs(&["list", "-H", "-t", "snapshot", "-o", "name", "-r", dataset])
        .lines()
        .map(str::to_string)
        .collect()
}

// --- the cases ------------------------------------------------------------

#[test]
fn the_auto_promote_fences_starts_its_guests_defers_the_heir_and_commits() {
    let c = Cluster::start("t2-auto");
    let out = c.promote("promote_auto", "node-b succeeds node-a");
    // Deferred at the heir, which only a person at node-c's console starts.
    let line = expect_exit("apply", &out, 5);
    let id = instance_of(&line);

    let mut running = jails();
    running.sort();
    assert_eq!(
        running,
        vec!["rue-t2-g1".to_string(), "rue-t2-g2".to_string()],
        "the guests are jails the kernel holds"
    );
    let actions = c.read("actions");
    assert!(
        actions.contains("fence node-a"),
        "the corpse was fenced: {actions}"
    );
    assert!(
        actions.contains("platform node-a slave"),
        "the resurrection gate held node-a in slave mode: {actions}"
    );
    assert!(
        actions.contains("placement node-b succeeds node-a"),
        "the placement was recorded: {actions}"
    );
    assert!(
        c.read("succession.log").contains("node-b succeeds node-a"),
        "the succession log is append-only and has the entry"
    );
    assert!(
        c.journal().contains("\"deferred\""),
        "the heir's deferral is journaled: {}",
        c.journal()
    );

    // The heir is up; the person says so, and the plan commits.
    let out = rue(&c.d.socket, &["handoff-done", &id, "--step", "8"]);
    let line = must("handoff-done", &out);
    assert!(line.to_lowercase().contains("committed"), "{line}");
    assert!(c.journal().contains("\"committed\""), "{}", c.journal());
}

#[test]
fn a_recant_after_the_guests_started_stops_each_one_and_records_the_reversal() {
    // Each guest is an iteration of a repeat whose undo names it:
    // `jail -r rue-t2-#{g}`. The engine undid a repeat's steps with no loop
    // variable at all, so this recant could not resolve the undo, left the
    // instance Stuck and both jails running. Nothing had reverted a promote
    // before this case.
    let c = Cluster::start("t2-recant");
    let out = c.promote("promote_auto", "node-b succeeds node-a");
    let line = expect_exit("apply", &out, 5);
    let id = instance_of(&line);
    let mut running = jails();
    running.sort();
    assert_eq!(
        running,
        vec!["rue-t2-g1".to_string(), "rue-t2-g2".to_string()]
    );

    must("recant", &rue(&c.d.socket, &["recant", &id]));
    assert!(
        jails().is_empty(),
        "each guest's jail was removed by its own undo: {:?}",
        jails()
    );
    let log = c.read("succession.log");
    assert!(
        log.contains("node-b succeeds node-a") && log.contains("reversed: node-b succeeds node-a"),
        "the append-only log keeps the entry and records its reversal: {log}"
    );
    let actions = c.read("actions");
    assert!(
        actions.contains("placement reversed: node-b succeeds node-a"),
        "the placement was compensated: {actions}"
    );
    assert!(
        actions.contains("platform node-a master"),
        "the resurrection gate let node-a out of slave mode: {actions}"
    );
    assert!(c.journal().contains("\"reverted\""), "{}", c.journal());
}

#[test]
fn a_refusal_after_the_fence_holds_under_auto_until_resumed() {
    // The placement service refuses this promote's entry: a refusal after
    // the fence, the guests already started, where a promote cannot go
    // back. The step's own compensation is recorded, so the log keeps the
    // entry and its reversal, and the instance holds rather than reverts.
    //
    // The obstacle-jail case, a start that fails on a jail someone else
    // made, is the next test's.
    let c = Cluster::start("t2-hold");
    c.set("placement_refuses", "held then resumed");
    let out = c.promote("promote_auto", "held then resumed");
    let line = expect_exit("apply", &out, 3);
    let id = instance_of(&line);
    let actions = c.read("actions");
    assert!(
        actions.contains("fence node-a"),
        "the hold is after the fence, where it matters: {actions}"
    );
    let mut running = jails();
    running.sort();
    assert_eq!(
        running,
        vec!["rue-t2-g1".to_string(), "rue-t2-g2".to_string()],
        "a hold keeps what was applied; it does not revert"
    );
    let log = c.read("succession.log");
    assert!(
        log.contains("held then resumed") && log.contains("reversed: held then resumed"),
        "the failed step's compensation is on the append-only log: {log}"
    );

    // The service recovers; resume retries the step that failed.
    c.set("placement_refuses", "");
    let out = rue(&c.d.socket, &["resume", &id]);
    expect_exit("resume", &out, 5);
    assert!(
        c.read("placement").contains("held then resumed"),
        "the placement was recorded on the retry"
    );
    let out = rue(&c.d.socket, &["handoff-done", &id, "--step", "8"]);
    let line = must("handoff-done", &out);
    assert!(line.to_lowercase().contains("committed"), "{line}");
}

/// The jail's id, as `jls` reports it; empty when there is no such jail.
fn jid(name: &str) -> String {
    let out = Command::new("jls")
        .args(["-j", name, "jid"])
        .output()
        .expect("jls");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_start_that_fails_on_a_jail_it_did_not_make_leaves_that_jail_and_holds() {
    // A jail already holds a guest's name, made by someone else. The guest's
    // start fails on it after the fence, and a failed step is undone at once
    // (5.9) -- by its undo, `jail -r rue-t2-g1`, which removes the jail of
    // that name whoever made it. It must not run: the engine reads the
    // guest's state through the probe that reads it (`jls` over ssh), the
    // state is the same before and after the failed start, so the start
    // never took and there is nothing to undo. The promote holds; with the
    // obstacle gone, resume starts the guest.
    let c = Cluster::start("t2-obstacle");
    let made = Command::new("jail")
        .args(["-c", "name=rue-t2-g1", "persist"])
        .status()
        .expect("jail");
    assert!(made.success(), "the obstacle jail");
    let planted = jid("rue-t2-g1");
    assert!(!planted.is_empty());

    let out = c.promote("promote_auto", "an obstacle in the way");
    let line = expect_exit("apply", &out, 3);
    let id = instance_of(&line);
    assert_eq!(
        jid("rue-t2-g1"),
        planted,
        "the failed start's undo removed or replaced a jail it never made"
    );
    assert!(
        c.journal().contains("\"undo_skipped\""),
        "the failed start is journaled as not undone: {}",
        c.journal()
    );
    assert!(
        c.read("actions").contains("fence node-a"),
        "the hold is after the fence"
    );

    // Whoever made the obstacle removes it; resume retries the start.
    let removed = Command::new("jail")
        .args(["-r", "rue-t2-g1"])
        .status()
        .expect("jail");
    assert!(removed.success());
    let out = rue(&c.d.socket, &["resume", &id]);
    expect_exit("resume", &out, 5);
    let mut running = jails();
    running.sort();
    assert_eq!(
        running,
        vec!["rue-t2-g1".to_string(), "rue-t2-g2".to_string()],
        "resume started both guests"
    );
    let out = rue(&c.d.socket, &["handoff-done", &id, "--step", "8"]);
    let line = must("handoff-done", &out);
    assert!(line.to_lowercase().contains("committed"), "{line}");
}

#[test]
fn a_second_promote_for_the_same_corpse_is_refused_with_75() {
    let c = Cluster::start("t2-exclusive");
    // The fence driver cannot tell, so the first promote waits at the knell
    // -- and while it waits it holds the corpse.
    c.set("fence", "unknown");
    let out = c.promote("promote_auto", "the first claimant");
    let line = expect_exit("apply", &out, 6);
    let first = instance_of(&line);

    let out = c.promote("promote", "the second claimant");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(75), "{text}");
    assert!(text.contains("R0101"), "{text}");
    assert!(
        !c.read("actions").contains("fence"),
        "nobody fenced node-a while two promotes contended for it"
    );
    must("recant", &rue(&c.d.socket, &["recant", &first]));
}

#[test]
fn the_manual_promote_acknowledges_both_knells_and_rolls_back_what_is_ahead() {
    let c = Cluster::start("t2-manual");
    // The corpse's dataset has moved on since the split: a write, and a
    // snapshot of it that a rollback to @split will destroy.
    let mnt = zfs(&["get", "-H", "-o", "value", "mountpoint", &c.dataset])
        .trim()
        .to_string();
    std::fs::write(format!("{mnt}/written-after-split"), "ahead\n").unwrap();
    zfs(&["snapshot", &format!("{}@late", c.dataset)]);

    let out = c.promote("promote", "node-b, by hand");
    let line = expect_exit("apply", &out, 6);
    let id = instance_of(&line);
    let journal = c.journal();
    assert!(
        journal.contains("fence_verdict: node-a: power off, confirmed by the fence driver"),
        "the fence's acknowledger was shown the measured verdict: {journal}"
    );

    // The fence, acknowledged by a human.
    let out = c.acknowledge(&id, 4, "the driver confirms node-a is off");
    expect_exit("ack the fence", &out, 6);
    assert!(c.read("actions").contains("fence node-a"));

    // The second knell: its cost is what `zfs rollback -r` destroys, read
    // off the real pool -- not the probe's name.
    let late = format!("{}@late", c.dataset);
    assert!(
        c.journal()
            .contains(&format!("destroyed_snapshots: {late}")),
        "the rollback's acknowledger was not shown what it destroys: {}",
        c.journal()
    );
    let out = c.acknowledge(&id, 5, "lose the late snapshot; the split is the truth");
    let line = expect_exit("ack the rollback", &out, 5);
    assert!(
        !snapshots(&c.dataset).contains(&late),
        "the rollback destroyed nothing: {:?}",
        snapshots(&c.dataset)
    );
    assert!(
        !std::path::Path::new(&format!("{mnt}/written-after-split")).exists(),
        "the write made after the split survived the rollback"
    );
    let _ = line;
    let out = rue(&c.d.socket, &["handoff-done", &id, "--step", "9"]);
    let line = must("handoff-done", &out);
    assert!(line.to_lowercase().contains("committed"), "{line}");
}

#[test]
fn a_write_between_the_request_and_the_acknowledgement_refuses_the_failback() {
    // Measured twice: the written-bytes guard is frozen at the request and
    // measured again when the acknowledgement arrives. Bytes written in
    // between are R0301 -- the plan refuses rather than acting on a picture
    // of the corpse that is no longer true.
    let c = Cluster::start("t2-twice");
    c.set("written", "0");
    let out = c.promote("promote", "on stale evidence");
    let line = expect_exit("apply", &out, 6);
    let id = instance_of(&line);

    c.set("written", "4096");
    let out = c.acknowledge(&id, 4, "the driver confirms node-a is off");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !text.to_lowercase().contains("applied") && !text.to_lowercase().contains("committed"),
        "the failback went ahead on a measurement that had changed: {text}"
    );
    assert!(
        c.journal().contains("host_contract_changed"),
        "the second measurement disagreed with the first and nothing said so: {}",
        c.journal()
    );
    assert!(
        !c.read("actions").contains("fence node-a"),
        "node-a was fenced on stale evidence"
    );
}
