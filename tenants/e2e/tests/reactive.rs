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
//!
//! Both drift variants of 8.4 run here, and they are the reason the
//! footprint of this tenant is worth having: every fact in it is an
//! appliance's reported state, read back through a hook and living in no
//! filesystem the engine can see. The engine compared drift over file
//! facts alone until this stage asked it to, so a hand-flipped actuator
//! was invisible -- `:clobber` overwrote a person's change without a word
//! and `:defer` never held anything at all.

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

/// Somebody at the panel, moving an actuator out from under the plan. The
/// hook reads these files, so writing one is exactly what a hand-flip is.
fn flip(state: &std::path::Path, name: &str, value: &str) {
    std::fs::write(state.join(name), format!("{value}\n")).expect("the panel");
}

fn journal(state: &std::path::Path) -> String {
    std::fs::read_to_string(state.join("journal.ndjson")).unwrap_or_default()
}

/// The world every case in this file starts from: a site, a state
/// directory, the two spawned children and a daemon.
fn world(name: &str) -> (Site, PathBuf, Daemon) {
    let me = rue_e2e::me();
    let site = Site::raw(name, &site_text(&me), INVENTORY);
    let state = site.dir.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::env::set_var("RUE_T4_STATE", &state);
    let sdk = rue_e2e::elixir_with_sdk().join(" ");
    let d = Daemon::start_with(&site, &spawned_children(&sdk));
    (site, state, d)
}

fn applied(out: &std::process::Output) -> String {
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        line.ends_with("Applied"),
        "the host did not apply: {line}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    line.split_whitespace().next().unwrap().to_string()
}

#[test]
fn a_hand_flipped_actuator_is_clobbered_under_clobber_and_journaled() {
    let (site, state, d) = world("t4-drift-clobber");
    let id = applied(&host(
        &d.socket,
        &state,
        &["enter", site.file.to_str().unwrap(), "shed_load"],
    ));
    assert_eq!(actuator(&state, "hvac-2"), "off");

    // A person turns one of them back on while the override stands. This
    // is drift on a `modified` fact that is not a file: nothing but the
    // appliance itself can be asked what it is now.
    flip(&state, "hvac-2", "manual");

    let out = host(&d.socket, &state, &["leave", &id]);
    assert!(
        out.status.success(),
        "recant: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        actuator(&state, "hvac-2"),
        "on",
        ":clobber restores over the change, as the plan declared"
    );
    assert_eq!(actuator(&state, "hvac-1"), "on", "and the rest reverts too");

    // Journaled, because a value the engine overwrote is exactly the thing
    // an operator has to be able to find afterwards.
    let j = journal(&state);
    assert!(
        j.contains("drift_clobbered"),
        "the drift was not journaled, so nobody can learn it happened: {j}"
    );
    assert!(
        j.contains("actuator:state:hvac-2"),
        "the entry does not name the fact that moved: {j}"
    );
}

#[test]
fn a_hand_flipped_actuator_holds_the_instance_under_defer_until_the_host_forces_it() {
    let (site, state, d) = world("t4-drift-defer");
    let id = applied(&host(
        &d.socket,
        &state,
        &["enter", site.file.to_str().unwrap(), "shed_load_deferring"],
    ));
    flip(&state, "pump-1", "manual");

    // The recant the host would ordinarily make does not revert: under
    // `:defer` an unattended undo does not overrule whoever moved it. The
    // verb succeeds and the instance lands in DriftHeld at exit 8 with
    // R0202 -- the drift is reported, not refused. It is the NEXT plain
    // recant that is refused (R0103), because by then the answer has been
    // given and repeating the question changes nothing.
    let out = host(&d.socket, &state, &["try-leave", &id]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let first: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("a recant outcome: {text}"));
    assert_eq!(first["state"], serde_json::json!("DriftHeld"));
    assert_eq!(first["exit"], serde_json::json!(8));
    assert!(
        first["line"].as_str().unwrap_or_default().contains("R0202"),
        "the outcome does not say what was held or why: {first}"
    );
    let out = host(&d.socket, &state, &["try-leave", &id]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("R0103"),
        "a second plain recant over deferred drift was not refused with R0103: {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        actuator(&state, "pump-1"),
        "manual",
        "the hand-flipped value stands"
    );
    assert_eq!(
        actuator(&state, "hvac-1"),
        "off",
        "and nothing of the step was undone around it"
    );

    let out = host(&d.socket, &state, &["status", &id]);
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).expect("a status document");
    assert_eq!(status["state"], serde_json::json!("DriftHeld"));
    assert_eq!(status["drift_held"], serde_json::json!([1]));
    assert_eq!(status["exit"], serde_json::json!(8));
    assert!(
        journal(&state).contains("drift_held"),
        "{}",
        journal(&state)
    );

    // The host forces it through over the same connection it applied on,
    // which is what "forced by the host itself" means in 8.4: no person at
    // a terminal, and no second channel.
    let out = host(&d.socket, &state, &["force", &id]);
    assert!(
        out.status.success(),
        "force: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(actuator(&state, "pump-1"), "on", "forced back to reported");
    assert_eq!(actuator(&state, "hvac-1"), "on");
    assert!(
        journal(&state).contains("drift_clobbered"),
        "forcing past a hold is still drift, and is still journaled: {}",
        journal(&state)
    );
}

#[test]
fn a_request_journals_and_reserves_nothing() {
    let (site, state, d) = world("t4-request");
    let out = host(
        &d.socket,
        &state,
        &["rehearse", site.file.to_str().unwrap(), "shed_load"],
    );
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "a rehearsal document: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let rehearsed = result["id"].as_str().expect("an id").to_string();

    // Nothing ran: the appliance never moved.
    assert_eq!(actuator(&state, "hvac-1"), "on");
    assert_eq!(actuator(&state, "hvac-2"), "on");
    assert_eq!(actuator(&state, "pump-1"), "on");

    // It journaled: a request is a thing the site is entitled to a record
    // of, and 8.4 asks for exactly that and nothing else.
    assert!(
        journal(&state).contains(&rehearsed),
        "the request was not journaled under its own id: {}",
        journal(&state)
    );
    let out = host(&d.socket, &state, &["status", &rehearsed]);
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).expect("a status");
    assert_eq!(status["rehearsal"], serde_json::json!(true));

    // And it reserved nothing, which is the half a journal cannot show. A
    // real apply of the same plan over the same three actuators would be
    // refused for contention if the rehearsal had taken an umbra; it is
    // admitted, so it did not.
    let id = applied(&host(
        &d.socket,
        &state,
        &["enter", site.file.to_str().unwrap(), "shed_load"],
    ));
    assert_eq!(actuator(&state, "hvac-1"), "off");
    let out = host(&d.socket, &state, &["leave", &id]);
    assert!(
        out.status.success(),
        "recant: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// T4's text with TWO journal sinks: a file the engine writes itself and a
/// hook that can be told to say no. 5.10 delivers to every declared sink
/// synchronously and all of them must acknowledge, which is what makes a
/// refusing sink stop a plan instead of merely losing an entry.
fn two_sink_text(me: &str) -> String {
    site_text(me)
        .replace(
            "journal to: hook(:host_log)",
            "journal to: file(\"kept.ndjson\"), hook(:host_log_refusing)",
        )
        // A daemon-spawned child is the socket owner and registers under
        // the registrar declared for it, so the name has to be one this
        // host may register or the sink never comes up at all (R0505).
        .replace(
            "may_register: [:host_world, :host_log, :host_actuate]",
            "may_register: [:host_world, :host_log_refusing, :host_actuate]",
        )
}

#[test]
fn a_sink_that_refuses_stops_the_plan_and_the_refusal_reaches_the_other_sink() {
    let me = rue_e2e::me();
    let site = Site::raw("t4-two-sinks", &two_sink_text(&me), INVENTORY);
    let state = site.dir.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::env::set_var("RUE_T4_STATE", &state);
    let sdk = rue_e2e::elixir_with_sdk().join(" ");
    // The refusing hook stands in for the ordinary journal child, and is
    // spawned before the inventory for the reason `boot_time_hooks` gives:
    // a registration is journaled, so a sink must be there to take it.
    let spawn: Vec<String> = ["host_world", "host_log_refusing"]
        .iter()
        .flat_map(|h| {
            [
                "--spawn".to_string(),
                format!("{h}={sdk} {}", fixture(&format!("{h}.exs")).display()),
            ]
        })
        .collect();
    let d = Daemon::start_with(&site, &spawn);

    // It was healthy at boot -- the daemon started, which is itself the
    // proof -- and now its storage goes away underneath it.
    let kept_before = std::fs::read_to_string(site.dir.join("kept.ndjson")).unwrap_or_default();
    let saw_before = std::fs::read_to_string(state.join("refusing-saw.ndjson")).unwrap_or_default();
    assert!(
        !kept_before.is_empty() && !saw_before.is_empty(),
        "both sinks must have taken the boot entries, or a refusal later proves \
         nothing about delivery: file sink {} bytes, hook sink {} bytes",
        kept_before.len(),
        saw_before.len()
    );
    std::fs::write(state.join("refuse"), "").unwrap();

    let out = host(
        &d.socket,
        &state,
        &["enter", site.file.to_str().unwrap(), "shed_load"],
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("R0304"),
        "a refusing sink did not stop the plan with R0304: {text}"
    );

    // The plan did not proceed: the write-ahead entry is acknowledged
    // before a step's `do` runs, so a refusal there means nothing ran.
    assert_eq!(actuator(&state, "hvac-1"), "on");
    assert_eq!(actuator(&state, "hvac-2"), "on");
    assert_eq!(actuator(&state, "pump-1"), "on");

    // The refusal reached the sink that still acknowledges. This is the
    // half worth having: an operator reading the surviving journal learns
    // that an entry was refused and by whom, rather than finding a plan
    // that stopped for no recorded reason.
    let kept = std::fs::read_to_string(site.dir.join("kept.ndjson")).expect("the file sink");
    assert!(
        kept.contains("R0304"),
        "the refusal never reached the other sink: {kept}"
    );
    assert!(
        kept.contains("host_log_refusing"),
        "the refusal does not name the sink that refused: {kept}"
    );

    // And both sinks really were delivered to: the hook was asked and said
    // no, rather than never being reached. It records every entry it is
    // handed, whether it accepts it or refuses it, so this counts what it
    // was sent and not what it kept.
    let saw = std::fs::read_to_string(state.join("refusing-saw.ndjson")).expect("what it saw");
    assert!(
        saw.lines().count() > saw_before.lines().count(),
        "the refusing sink was never delivered to, so the plan stopped for \
         some other reason: {} lines before, {} after",
        saw_before.lines().count(),
        saw.lines().count()
    );
}
