//! The Phase 2 performance acceptance (docs/ROADMAP.md 1196): `rue check`
//! of a 200-step plan against a 1,000-host inventory, parse and resolve
//! and check, under 2 s on the CI image and 5 s on FreeBSD. The bound is
//! asserted as the acceptance names it, in release (the gate runs tests
//! in release); a debug build asserts twice the bound so the test still
//! runs on a workstation `cargo test` and in the rediscovery battery, and
//! says so. The measured time is tens of milliseconds either way.

use std::fs;
use std::time::{Duration, Instant};

use rue_core::check::check;
use rue_surface::resolve::{resolve, Options};

fn bound() -> Duration {
    let base = if cfg!(target_os = "freebsd") { 5 } else { 2 };
    let factor = if cfg!(debug_assertions) { 2 } else { 1 };
    Duration::from_secs(base * factor)
}

#[test]
fn a_200_step_plan_over_1000_hosts_checks_within_the_acceptance_bound() {
    let dir = std::env::temp_dir().join(format!("rue-bench-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut inv = String::new();
    for i in 0..1000 {
        inv.push_str(&format!(
            "[[host]]\nname = \"h{i}\"\naddress = \"10.0.{}.{}\"\nos = \"freebsd\"\nroles = [\"db\"]\nreach = [\"ssh\"]\nfilesystem = true\nscheduler = \"cron\"\n\n",
            i / 256,
            i % 256
        ));
    }
    inv.push_str("[authenticators]\noncall = { human = true }\n");
    fs::write(dir.join("inventory.toml"), inv).unwrap();
    let mut text = String::from(
        "rue 0\nsite do\n  inventory from: file(\"inventory.toml\")\n  journal to: local()\n  backstop scheduler: cron()\n  operators do\n    identity :requester, user: \"ops\", operator_for: :all, admin: true\n  end\nend\n",
    );
    for i in 0..200 {
        text.push_str(&format!(
            "defop :op{i}, %{{os: :freebsd}} do\n  footprint owned: file(\"/etc/rue/{i}\"), derived: probe{i}\n  do: [write(file(\"/etc/rue/{i}\"), content: posture), run(\"service svc{i} reload\")]\n  undo: :restore\n  undo_locus: :target\nend\n"
        ));
    }
    text.push_str("defplan :big, %{os: :freebsd} do\n  wane 4h, renew_within: 30m\n  backstop trigger: [after: 4h, unless_heartbeat: 60s, interval: 20s], locus: :target, arm_before: 1\n");
    for i in 0..200 {
        text.push_str(&format!("  op{i}(posture: \"x\")\n"));
    }
    text.push_str("end\n");
    let plan = dir.join("plan.rue");
    fs::write(&plan, text).unwrap();
    let opts = Options {
        suspend_e0604: false,
        host: Some("h999".into()),
        plan: None,
        requester: None,
        inventory: None,
    };
    let start = Instant::now();
    let ir = resolve(&plan, &opts).unwrap_or_else(|d| {
        panic!(
            "{}",
            d.iter().map(|d| d.render()).collect::<Vec<_>>().join("\n")
        )
    });
    let v = check(&ir.site, &ir.requester, &ir.plan);
    let elapsed = start.elapsed();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        v.status,
        rue_core::verdict::Status::Ok,
        "{:?}",
        v.diagnostics
    );
    assert_eq!(ir.site.hosts.len(), 1000);
    assert!(
        elapsed <= bound(),
        "parse, resolve and check took {elapsed:?}; the bound is {:?} ({})",
        bound(),
        if cfg!(debug_assertions) {
            "debug, twice the acceptance bound"
        } else {
            "the acceptance bound"
        }
    );
    eprintln!(
        "bench: 200 steps over 1000 hosts in {elapsed:?} (bound {:?})",
        bound()
    );
}
