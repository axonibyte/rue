//! Tier 6: what happens when the engine dies mid-flight, when the daemon
//! is gone at the moment the backstop matters, and when an operator and a
//! target race each other.
//!
//! Every case here kills something. What is asserted afterwards is what an
//! operator would look at: the world on the host, the instance the store
//! holds, and the journal.

use std::time::Duration;

use rue_e2e::{
    bin, instance_of, must, require_provisioned_host, rue, rue_root, target_exists, target_read,
    target_run, target_write, Daemon, Site,
};

/// Two owned files on the target: the second step is slow enough that a
/// kill lands in the middle of the plan.
fn plans(a: &str, b: &str) -> String {
    format!(
        r##"
defop :own_a, _ do
  footprint owned: file("{a}")
  do: write(file("{a}"), content: "a\n")
  undo: :restore
  undo_locus: :target
end

defop :own_b, _ do
  footprint owned: file("{b}")
  do: [run("sleep 5"), write(file("{b}"), content: "b\n")]
  undo: :restore
  undo_locus: :target
end

defplan :two_steps, _ do
  wane 1h, renew_within: 10m
  own_a()
  own_b()
end
"##
    )
}

#[test]
fn a_daemon_killed_mid_apply_reverts_what_it_had_applied_when_it_comes_back() {
    require_provisioned_host();
    let (a, b) = ("/etc/rue-e2e-kill-a", "/etc/rue-e2e-kill-b");
    for f in [a, b] {
        let _ = std::process::Command::new("rm").arg("-f").arg(f).status();
    }
    let site = Site::new("recovery-kill", &plans(a, b));
    let d = Daemon::start(&site);
    // Apply in the background; the second step sleeps, so the kill lands
    // between the first step's completion and the second's.
    let socket = d.socket.clone();
    let file = site.file.clone();
    let applying = std::thread::spawn(move || {
        std::process::Command::new(bin("rue"))
            .arg("apply")
            .arg(&file)
            .arg("--host")
            .arg("fw-01")
            .arg("--socket")
            .arg(&socket)
            .output()
    });
    // Wait for the first step to land on the target, then kill the daemon
    // as hard as the operating system allows.
    let start = std::time::Instant::now();
    while !target_exists(a) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the first step never landed"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let pid = d.pid();
    let _ = std::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
    let _ = applying.join();
    // The world is left as the kill found it: the first step applied.
    assert!(target_exists(a), "the first step is on the host");
    // The daemon comes back: boot demotes the instance it was applying and
    // reverts it (7.8).
    let mut d = Daemon::start(&site);
    let start = std::time::Instant::now();
    while target_exists(a) {
        if start.elapsed() >= Duration::from_secs(60) {
            panic!(
                "boot recovery never reverted the applied step.\n--- rued said ---\n{}\n--- journal ---\n{}",
                d.said(),
                std::fs::read_to_string(site.dir.join("journal.ndjson")).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(!target_exists(b), "the second step never completed");
    let journal = std::fs::read_to_string(site.dir.join("journal.ndjson")).unwrap_or_default();
    assert!(
        journal.contains("reverting") && journal.contains("reverted"),
        "the revert is journaled: {journal}"
    );
    d.stop();
}

#[test]
fn a_backstop_armed_by_a_daemon_that_dies_still_fires_from_the_target_s_own_cron() {
    require_provisioned_host();
    let f = "/etc/rue-e2e-canary";
    let _ = std::process::Command::new("rm").arg("-f").arg(f).status();
    // A temporary plan whose wane is already in the past by the time cron
    // next runs: the artifact fires on its own, with no engine anywhere.
    let plans = format!(
        r##"
defop :own_canary, _ do
  footprint owned: file("{f}")
  do: write(file("{f}"), content: "canary\n")
  undo: :restore
  undo_locus: :target
end

defplan :canary, _ do
  wane 1m, renew_within: 30s
  backstop trigger: [after: 1m], locus: :target, arm_before: 1
  own_canary()
end
"##
    );
    let site = Site::new("recovery-canary", &plans);
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    let id = instance_of(&must("apply", &out));
    assert!(target_exists(f), "the step applied");
    // The artifact and its deadline are on the target.
    let dir = rue_root().join("instances").join(&id);
    assert!(
        target_exists(dir.join("artifact.sh").to_str().unwrap()),
        "the artifact is installed at {}",
        dir.display()
    );
    // The daemon dies. Nothing but cron and the artifact remain.
    let pid = d.pid();
    let _ = std::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
    // cron runs the entry every minute; the deadline is a minute out. Two
    // ticks is the bound, plus a little for the minute boundary.
    let start = std::time::Instant::now();
    while !target_exists(dir.join("fired").to_str().unwrap()) {
        assert!(
            start.elapsed() < Duration::from_secs(200),
            "the artifact never fired; the crontab says: {}",
            target_read("/dev/null")
        );
        std::thread::sleep(Duration::from_secs(2));
    }
    assert!(
        !target_exists(f),
        "the artifact undid the step with no engine in sight"
    );
}

#[test]
fn a_recant_and_a_fired_artifact_do_not_undo_the_same_step_twice() {
    require_provisioned_host();
    let f = "/etc/rue-e2e-race";
    target_write(f, "before\n");
    // The step modifies a file; both the engine and the artifact restore
    // it from the same snapshot under the same host lock, so whichever
    // wins, the end state is the file as the step found it.
    let plans = format!(
        r##"
defop :modify, _ do
  footprint modified: file("{f}")
  do: write(file("{f}"), content: "after\n")
  undo: :restore
  undo_locus: :target
end

defplan :racing, _ do
  wane 1m, renew_within: 30s
  backstop trigger: [after: 1m], locus: :target, arm_before: 1
  modify()
end
"##
    );
    let site = Site::new("recovery-race", &plans);
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    let id = instance_of(&must("apply", &out));
    assert_eq!(target_read(f), "after\n");
    // Recant while the artifact's deadline is passing: the host lock is
    // the only thing between them.
    let out = rue(&d.socket, &["recant", &id]);
    must("recant", &out);
    assert_eq!(target_read(f), "before\n", "restored once, not twice");
    // Give cron a chance to fire the artifact if it was left armed; the
    // file must still be what the step found.
    std::thread::sleep(Duration::from_secs(70));
    assert_eq!(
        target_read(f),
        "before\n",
        "a fired artifact would restore the same snapshot, not another"
    );
    d.stop();
}

#[test]
fn doctor_with_a_canary_proves_a_real_backstop_fires() {
    require_provisioned_host();
    // The one proof no unit test can give: this host's cron runs what rue
    // installs. A throwaway artifact, armed with a deadline already past,
    // and removed whatever happens.
    let site = Site::new("recovery-canary-doctor", "");
    let d = Daemon::start(&site);
    let out = rue(&d.socket, &["doctor", "--canary", "--canary-wait", "200"]);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        text.contains("fired"),
        "the canary reported: {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "the canary fired: {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // It left nothing behind: no instance directory of its own.
    let dirs = target_run(&format!(
        "ls {} 2>/dev/null || true",
        rue_root().join("instances").display()
    ));
    assert!(
        !dirs.contains("rue-canary-"),
        "the canary cleaned up after itself: {dirs}"
    );
    d.stop();
}

#[test]
fn doctor_reports_the_host_the_bindings_and_the_bootstrap() {
    require_provisioned_host();
    let site = Site::new("recovery-doctor", "");
    let d = Daemon::start(&site);
    let out = rue(&d.socket, &["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "doctor: {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("fw-01"), "the host is named: {text}");
    assert!(text.contains("ssh"), "its transport is named: {text}");
    // The guest is bootstrapped by provisioning, so doctor says so.
    assert!(text.contains("cron"), "its scheduler is named: {text}");
    d.stop();
}
