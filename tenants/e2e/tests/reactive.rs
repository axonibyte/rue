//! T4 end to end (docs/ROADMAP.md 8.4): a reactive host that embeds rue.
//!
//! This is the stage the whole phase is for. A single Elixir process holds
//! one control-channel connection and is three things at once on it: the
//! hooks the engine calls back into (`host_log`, `host_actuate`), the
//! declared operator `:reactive_host` issuing verbs, and a subscriber to
//! its own plans. Its inventory comes from a fourth hook spawned as a
//! child, because `inventory.list` is asked at boot before the socket
//! serves and nothing registered over the socket could answer it.
//!
//! What is proven here that no unit test can prove: the same `.rue` text
//! yields the same verdict whether a person checks it at a terminal or a
//! host applies it over a channel, and a temporary plan an embedded host
//! fired really does revert.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use rue_e2e::{repo_root, rue, Daemon, Site};

const INVENTORY: &str = r#"# What hook(:host_world) reports, kept beside the text so `rue check
# --inventory` has a record to check against (E0607).
[[host]]
name = "site-ctl"
address = "127.0.0.1"
os = "reactive-host"
roles = ["controller"]
reach = ["actuate"]
filesystem = false

[authenticators]
site_operator = { human = true }
"#;

/// T4's text, as the tenant carries it, with the subscription the host
/// needs to see its own plan's entries.
fn site_text(me: &str) -> String {
    format!(
        r#"rue 0
site do
  inventory from: hook(:host_world)
  journal to: hook(:host_log)
  execute via: hook(:host_actuate, transport: :actuate)
  operators do
    identity :reactive_host, user: "{me}", operator_for: [:shed_load, :shed_load_deferring], admin: true, subscribe: [:shed_load, :shed_load_deferring]
  end
  hooks do
    registrar :reactive_host, user: :socket_owner, may_register: [:host_world, :host_log, :host_actuate]
  end
end

defop :shed_load, _, drift: drift do
  footprint modified: actuator.state("hvac-1"), modified: actuator.state("hvac-2"), modified: actuator.state("pump-1")
  do: hook(:host_actuate, set: %{{"hvac-1": :off, "hvac-2": :off, "pump-1": :low}})
  undo: :restore
  undo_locus: :controller
  drift: drift
end

defplan :shed_load, %{{name: "site-ctl"}} do
  wane 2h
  shed_load(drift: :clobber)
end

defplan :shed_load_deferring, %{{name: "site-ctl"}} do
  wane 2h
  shed_load(drift: :defer)
end

# A plan the reactive host is deliberately NOT scoped to, so that a scope
# violation has something to violate (R0504). Nothing else uses it.
defplan :not_mine, %{{name: "site-ctl"}} do
  wane 2h
  shed_load(drift: :clobber)
end
"#
    )
}

fn fixture(name: &str) -> PathBuf {
    repo_root().join("tenants/t4/fixtures").join(name)
}

/// The reactive host, run for one command, with the SDK on its code path.
fn host(socket: &std::path::Path, state: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut c = Command::new(&rue_e2e::elixir_with_sdk()[0]);
    for a in &rue_e2e::elixir_with_sdk()[1..] {
        c.arg(a);
    }
    c.arg(fixture("reactive_host.exs"))
        .arg(socket)
        .args(args)
        .env("RUE_T4_STATE", state)
        .env("RUE_BIN", rue_e2e::bin("rue"))
        .output()
        .expect("the reactive host runs")
}

/// The two hooks a boot-time binding names, which must be children: the
/// inventory is asked before the socket serves and the journal is written
/// by boot recovery itself. The reactive host serves the third over its
/// own connection and watches the journal through its subscription.
fn spawned_children(sdk: &str) -> Vec<String> {
    ["host_world", "host_log"]
        .iter()
        .flat_map(|h| {
            [
                "--spawn".to_string(),
                format!("{h}={sdk} {}", fixture(&format!("{h}.exs")).display()),
            ]
        })
        .collect()
}

fn actuator(state: &std::path::Path, name: &str) -> String {
    std::fs::read_to_string(state.join(name))
        .unwrap_or_else(|_| "on\n".into())
        .trim()
        .to_string()
}

#[test]
fn a_reactive_host_fires_a_plan_and_recants_it_over_its_own_connection() {
    let me = rue_e2e::me();
    let site = Site::raw("t4", &site_text(&me), INVENTORY);
    let state = site.dir.join("state");
    std::fs::create_dir_all(&state).unwrap();

    // The inventory hook is a spawned child: the engine asks it at boot,
    // before the socket serves.
    // The spawned children inherit the daemon's environment, and the
    // daemon inherits this process's, so the state directory is set here
    // rather than on each invocation.
    std::env::set_var("RUE_T4_STATE", &state);
    let sdk = rue_e2e::elixir_with_sdk().join(" ");
    let spawn = spawned_children(&sdk);
    let d = Daemon::start_with(&site, &spawn);

    // 1. The verdict a person gets at a terminal.
    // Not through the harness's `rue()`: that adds `--socket`, which is
    // for the verbs that talk to a daemon, and `check` talks to nobody.
    let standalone = Command::new(rue_e2e::bin("rue"))
        .args([
            "check",
            site.file.to_str().unwrap(),
            "--host",
            "site-ctl",
            "--plan-name",
            "shed_load",
            "--inventory",
            site.dir.join("inventory.toml").to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("rue check");
    assert_eq!(
        standalone.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&standalone.stderr)
    );
    let standalone: serde_json::Value =
        serde_json::from_slice(&standalone.stdout).expect("a verdict");

    // 2. The host enters its state and fires the plan.
    let out = host(
        &d.socket,
        &state,
        &["enter", site.file.to_str().unwrap(), "shed_load"],
    );
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        line.ends_with("Applied"),
        "the host did not apply: {line}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let id = line.split_whitespace().next().unwrap().to_string();

    // The actuators really moved, through the host's own execute hook.
    assert_eq!(actuator(&state, "hvac-1"), "off");
    assert_eq!(actuator(&state, "hvac-2"), "off");
    assert_eq!(actuator(&state, "pump-1"), "low");

    // The host's journal hook really received the entries.
    let journal =
        std::fs::read_to_string(state.join("journal.ndjson")).expect("the host's journal");
    assert!(
        journal.contains("\"Applied\"") || journal.contains("applied"),
        "{journal}"
    );

    // 3. The acceptance line of the phase: the same `.rue` text checks
    // identically standalone and embedded. The person ran `rue check` on
    // the text; the host asked `rue check --ir` for the same text and sent
    // that IR, which the daemon checked again before admitting it. So the
    // two checks are of one document, and what is asserted here is that
    // they agree about the plan, not merely that both succeeded.
    assert_eq!(standalone["diagnostics"], serde_json::json!([]));
    assert_eq!(standalone["plan"], serde_json::json!("shed_load"));
    assert_eq!(standalone["host"], serde_json::json!("site-ctl"));
    assert_eq!(standalone["intent"], serde_json::json!("temporary"));
    assert_eq!(standalone["wane_s"], serde_json::json!(7200));
    assert_eq!(
        standalone["backstop"],
        serde_json::Value::Null,
        "no backstop"
    );
    assert_eq!(
        standalone["controller_only_undos"],
        serde_json::json!([1]),
        "step 1 reverts only while the engine lives (undo_locus: :controller)"
    );

    // And the daemon, which checked the IR rather than the text, admitted
    // the same plan on the same host.
    let embedded = rue(&d.socket, &["status", &id, "--identity", "reactive_host"]);
    assert_eq!(embedded.status.code(), Some(0));
    let status = String::from_utf8_lossy(&embedded.stdout).into_owned();
    assert!(
        status.contains("shed_load") && status.contains("site-ctl"),
        "the daemon's view names the same plan and host: {status}"
    );

    // 4. Leaving the state recants, and the actuators come back.
    let out = host(&d.socket, &state, &["leave", &id]);
    assert!(
        out.status.success(),
        "recant: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        actuator(&state, "hvac-1"),
        "on",
        "the undo restored what was reported"
    );
    assert_eq!(actuator(&state, "pump-1"), "on");
}

#[test]
fn the_host_is_refused_outside_its_registrar_and_outside_its_scope() {
    let me = rue_e2e::me();
    let site = Site::raw("t4-refusals", &site_text(&me), INVENTORY);
    let state = site.dir.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::env::set_var("RUE_T4_STATE", &state);
    let sdk = rue_e2e::elixir_with_sdk().join(" ");
    let d = Daemon::start_with(&site, &spawned_children(&sdk));

    // R0505: a name outside `may_register`. The registrar declaration is
    // what a hook name is trusted by, and it is not a list this host may
    // extend by asking.
    let out = host(&d.socket, &state, &["rogue-register"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("R0505"),
        "a name outside may_register was not refused with R0505: {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // R0504: an act on a plan outside `operator_for`. The identity is
    // scoped to its two plans by name, and :not_mine is not one of them --
    // which is the whole of the scope model: an embedded host is a control
    // client with a declared identity, and every act it makes is bounded
    // by that declaration rather than by what it can reach.
    let out = host(
        &d.socket,
        &state,
        &["enter", site.file.to_str().unwrap(), "not_mine"],
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("R0504"),
        "applying a plan outside operator_for was not refused with R0504: {text}"
    );
}
