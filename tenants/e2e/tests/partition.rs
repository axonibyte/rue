//! Tier 6: `unless_heartbeat` under a real severed link (ROADMAP 5.6 and
//! Phase 5's acceptance; `docs/issues/0002`).
//!
//! Every other proof of the dead-man trigger has been one machine's clocks
//! agreeing with themselves: the engine stopped writing the heartbeat
//! because it was killed, not because it could not reach the target. Here
//! the engine is alive and well and the path to the target is cut, which is
//! the case the trigger exists for -- a controller that can no longer
//! reach a host cannot undo anything on it, and the target has to act on
//! its own clock or not at all.
//!
//! The cut is a firewall rule in the anchor `provision.sh` declares empty,
//! naming the target address and port 22 alone; the management interface
//! reaper watches over is skipped by the baseline and is never in it. While
//! the cut holds, the target is read through the filesystem rather than
//! through `target_read`, because the harness runs on the target and ssh to
//! it is precisely what has been severed.

use std::fs;
use std::time::{Duration, Instant};

use rue_e2e::{
    instance_of, must, require_provisioned_host, restore_target, rue, rue_root, sever_target,
    target_reachable, Daemon, Site,
};

/// A plan whose backstop is the dead man alone: `after:` is an hour out, so
/// nothing fires by time inside the stage, and `unless_heartbeat: 60s`
/// beats every 20s (a third of its deadline, ROADMAP 5.6) and fires once
/// the last beat is a minute old.
fn plans(f: &str) -> String {
    format!(
        r##"
defop :own_it, _ do
  footprint owned: file("{f}")
  do: write(file("{f}"), content: "alive\n")
  undo: :restore
  undo_locus: :target
end

defplan :dead_man, _ do
  wane 1h, renew_within: 10m
  backstop trigger: [after: 1h, unless_heartbeat: 60s], locus: :target, arm_before: 1
  own_it()
end
"##
    )
}

/// Restores the link however the stage leaves: a severed guest would fail
/// every stage after this one, and a panic must not be able to do that.
struct Severed;

impl Drop for Severed {
    fn drop(&mut self) {
        restore_target();
    }
}

fn wait_for<F: Fn() -> bool>(what: &str, secs: u64, f: F) {
    let start = Instant::now();
    while !f() {
        assert!(
            start.elapsed() < Duration::from_secs(secs),
            "{what} did not happen within {secs}s"
        );
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[test]
fn a_severed_controller_leaves_the_target_to_undo_the_plan_on_its_own_clock() {
    require_provisioned_host();
    let f = "/etc/rue-e2e-partition";
    let _ = fs::remove_file(f);
    let site = Site::new("partition", &plans(f));
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    let id = instance_of(&must("apply", &out));
    assert_eq!(fs::read_to_string(f).unwrap_or_default(), "alive\n");

    // The artifact is installed and the engine is beating.
    let dir = rue_root().join("instances").join(&id);
    assert!(dir.join("artifact.sh").exists(), "{} ", dir.display());
    wait_for("the first heartbeat", 60, || dir.join("heartbeat").exists());
    let beat = fs::read_to_string(dir.join("heartbeat")).unwrap_or_default();

    // Cut the path. The daemon stays up and keeps trying; what it cannot
    // do is reach the host.
    let severed = Severed;
    sever_target();
    assert!(
        !target_reachable(),
        "the link is severed: ssh to the target must not answer"
    );

    // The heartbeat stops advancing, which is the whole of the signal the
    // target has. Two intervals is enough to tell a stopped beat from a
    // slow one.
    std::thread::sleep(Duration::from_secs(45));
    assert_eq!(
        fs::read_to_string(dir.join("heartbeat")).unwrap_or_default(),
        beat,
        "the engine cannot reach the host, so the beat it wrote before the cut is the last"
    );

    // cron runs the artifact every minute; it fires on the first tick that
    // finds the beat older than its deadline, with no engine involved.
    wait_for("the artifact firing on a stale heartbeat", 200, || {
        dir.join("fired").exists()
    });
    assert!(
        !std::path::Path::new(f).exists(),
        "the target undid the step with the controller unreachable"
    );

    // The link comes back, and the engine reads the firing on its next
    // contact: R0402, journaled per step the target undid.
    drop(severed);
    wait_for("the target answering again", 60, target_reachable);
    let journal = site.dir.join("journal.ndjson");
    wait_for("the engine reading the firing", 120, || {
        fs::read_to_string(&journal)
            .unwrap_or_default()
            .contains("backstop_fired")
    });
    let text = fs::read_to_string(&journal).unwrap_or_default();
    assert!(
        text.contains("\"step\":1") || text.contains("\"step\": 1"),
        "the step the target undid is named: {text}"
    );
    d.stop();
}
