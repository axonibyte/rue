//! rued and rue end to end over a real socket in a temporary directory:
//! the daemon serves a site file, a hook registers over the channel and
//! serves execute, `rue apply` runs a plan through it and prints the
//! verdict line last with the outcome's exit code, `rue status` and `rue
//! recant` act on it, `--dry-run` rehearses, a spawned child over stdio
//! registers as the socket owner, and daemon dry-run mode suspends E0604.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rue_engine::control::Client;
use rue_engine::hook::{Registration, HOOK_PROTOCOL};
use rue_engine::peer::{my_uid, user_name};
use serde_json::{json, Value};

fn rue_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rue"))
}

/// The daemon binary beside the CLI's, built fresh by this test run (a
/// `cargo test -p rue` builds only rue; a stale rued would test old code),
/// once per process.
fn rued_bin() -> PathBuf {
    static BUILT: std::sync::Once = std::sync::Once::new();
    let p = rue_bin().with_file_name("rued");
    BUILT.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut c = Command::new(cargo);
        c.args(["build", "-p", "rued", "--locked"]);
        if rue_bin().to_string_lossy().contains("/release/") {
            c.arg("--release");
        }
        let out = c.output().expect("cargo build -p rued");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    });
    assert!(p.exists(), "no rued at {}", p.display());
    p
}

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "rue-d-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 1_000_000_000
        ));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn my_gid() -> u32 {
    // SAFETY: getegid has no preconditions.
    unsafe { libc::getegid() }
}

const INVENTORY: &str = r#"
[[host]]
name = "h"
address = "10.0.0.1"
os = "freebsd"
reach = ["api"]
filesystem = false

[authenticators]
oncall = { human = true }
"#;

fn site(me: &str, with_operators: bool) -> String {
    // Dry-run mode suspends E0604 (the operators block) and nothing else:
    // the registrar the execute hook needs (E0605) stays.
    let ops = if with_operators {
        format!(
            "  operators do\n    identity :ops, user: \"{me}\", operator_for: :all, admin: true, subscribe: [:p]\n    identity :owner, user: :socket_owner, operator_for: [:p]\n  end\n  hooks do\n    registrar :owner, user: :socket_owner, may_register: [:act]\n  end\n"
        )
    } else {
        "  hooks do\n    registrar :owner, user: :socket_owner, may_register: [:act]\n  end\n"
            .to_string()
    };
    format!(
        "rue 0\nsite do\n  inventory from: file(\"inventory.toml\")\n  journal to: file(\"journal.ndjson\")\n  execute via: hook(:act, transport: :api)\n{ops}end\n"
    )
}

const PLAN: &str = r#"
defop :poke, _ do
  footprint owned: file("/tmp/poke")
  do run("poke")
  undo run("unpoke", idempotent: true)
  undo_pre file("/tmp/poke")
  undo_locus :controller
end

defplan :p, %{name: "h"} do
  wane 1h
  poke()
end
"#;

struct Daemon {
    child: Child,
    socket: PathBuf,
}

impl Daemon {
    fn start(dir: &Path, site_file: &Path, extra: &[&str]) -> Daemon {
        let socket = dir.join("rued.sock");
        let mut c = Command::new(rued_bin());
        c.arg("run")
            .arg("--site")
            .arg(site_file)
            .arg("--store")
            .arg(dir.join("store"))
            .arg("--socket")
            .arg(&socket)
            .arg("--group")
            .arg(my_gid().to_string())
            .arg("--hook-deadline")
            .arg("2")
            .arg("--reap-every")
            .arg("1")
            .args(extra)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let child = c.spawn().expect("rued");
        let d = Daemon { child, socket };
        let start = Instant::now();
        while !d.socket.exists() || UnixStream::connect(&d.socket).is_err() {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "rued never served its socket"
            );
            thread::sleep(Duration::from_millis(50));
        }
        d
    }

    fn stop(mut self) -> String {
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

fn rue(socket: &Path, args: &[&str]) -> std::process::Output {
    Command::new(rue_bin())
        .args(args)
        .arg("--socket")
        .arg(socket)
        .output()
        .expect("rue")
}

/// A hook over the channel serving execute: answers every run ok, on its
/// own thread, until the connection ends.
fn serve_hook(socket: &Path, name: &str) -> thread::JoinHandle<Vec<Value>> {
    let mut c = Client::connect(socket).unwrap();
    c.hello(Some("owner")).unwrap();
    c.register(&Registration {
        name: name.into(),
        kinds: vec!["execute".into()],
        protocol: HOOK_PROTOCOL,
        filesystem: false,
        stdin_preamble: false,
    })
    .unwrap();
    thread::spawn(move || {
        let mut served = Vec::new();
        while let Ok(req) = c.next_frame() {
            if req.get("event").is_some() {
                continue;
            }
            let id = req["id"].clone();
            served.push(req);
            let reply = json!({ "id": id, "ok": true, "output": { "stdout": "", "outputs": {} }, "facts": [] });
            if c.send(&reply).is_err() {
                break;
            }
        }
        served
    })
}

/// A site whose hosts come from a hook, not a file.
fn hook_inventory_site(me: &str) -> String {
    let mut t = String::from("rue 0\nsite do\n");
    t.push_str("  inventory from: hook(:world)\n");
    t.push_str("  journal to: file(\"journal.ndjson\")\n");
    t.push_str("  execute via: hook(:world, transport: :api)\n");
    t.push_str("  operators do\n");
    t.push_str(&format!(
        "    identity :ops, user: \"{me}\", operator_for: :all, admin: true\n"
    ));
    t.push_str("  end\n  hooks do\n");
    t.push_str("    registrar :ops, user: :socket_owner, may_register: [:world]\n");
    t.push_str("  end\nend\n");
    t
}

#[test]
fn a_daemon_takes_its_hosts_from_the_inventory_hook_and_says_so() {
    // The hook is asked once, after its child has registered and before
    // boot recovery, because reconciliation needs the hosts to reconcile
    // against. Nothing here reads a file for them: the record beside the
    // text exists only so the text can be *checked*, which is a separate
    // question from what the daemon believes at run time.
    let d = TempDir::new("hook-inv");
    let me = user_name(my_uid()).unwrap();
    fs::write(d.0.join("inventory.toml"), INVENTORY).unwrap();
    let site_file = d.0.join("plan.rue");
    fs::write(&site_file, format!("{}{PLAN}", hook_inventory_site(&me))).unwrap();
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/inventory-hook.sh"
    );
    let daemon = Daemon::start(
        &d.0,
        &site_file,
        &["--spawn", &format!("world=sh {fixture} world")],
    );

    // The plan applies on a host only the hook named. `--inventory` is the
    // record the *check* is made against (E0607 without one).
    let out = rue(
        &daemon.socket,
        &[
            "apply",
            site_file.to_str().unwrap(),
            "--host",
            "h",
            "--identity",
            "ops",
            "--inventory",
            d.0.join("inventory.toml").to_str().unwrap(),
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = daemon.stop();
    assert!(
        err.contains("inventory from hook world: 1 hosts"),
        "the daemon does not say where its hosts came from:\n{err}"
    );
}

#[test]
fn a_daemon_whose_inventory_hook_never_registers_refuses_to_start() {
    // The failure to avoid is a daemon that boots with no hosts and
    // reports every plan unreachable, which looks like the site is broken
    // rather than like the hook is missing.
    let d = TempDir::new("hook-inv-absent");
    let me = user_name(my_uid()).unwrap();
    fs::write(d.0.join("inventory.toml"), INVENTORY).unwrap();
    let site_file = d.0.join("plan.rue");
    fs::write(&site_file, format!("{}{PLAN}", hook_inventory_site(&me))).unwrap();
    let out = Command::new(rued_bin())
        .arg("run")
        .arg("--site")
        .arg(&site_file)
        .arg("--store")
        .arg(d.0.join("store"))
        .arg("--socket")
        .arg(d.0.join("rued.sock"))
        .arg("--group")
        .arg(my_gid().to_string())
        .output()
        .expect("rued");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("hook world") && err.contains("--inventory"),
        "the refusal must name the hook and the way out:\n{err}"
    );
}

#[test]
fn an_event_arriving_while_a_verb_is_in_flight_is_kept_and_not_discarded() {
    // `:ops` subscribes to `:p`, so applying p sends this connection the
    // plan's journal entries -- on the same socket the reply comes back
    // on, interleaved with it. Reading past an event to reach the reply is
    // unavoidable; discarding it is not. A host that subscribes to a plan
    // and also drives it (T4's shape) would otherwise lose exactly the
    // events it asked for, and only when it happened to be mid-verb, which
    // is the hardest kind of loss to notice.
    let d = TempDir::new("events");
    let me = user_name(my_uid()).unwrap();
    fs::write(d.0.join("inventory.toml"), INVENTORY).unwrap();
    let site_file = d.0.join("plan.rue");
    fs::write(&site_file, format!("{}{PLAN}", site(&me, true))).unwrap();
    let daemon = Daemon::start(&d.0, &site_file, &[]);
    let _hook = serve_hook(&daemon.socket, "act");

    let mut c = Client::connect(&daemon.socket).unwrap();
    c.hello(Some("ops")).unwrap();
    assert_eq!(c.pending_events(), 0, "nothing has happened yet");

    // Another connection applies p. This one is a subscriber, so the
    // daemon writes p's entries to its socket as they happen -- they are
    // sitting in front of the next reply it reads.
    let out = rue(
        &daemon.socket,
        &[
            "apply",
            site_file.to_str().unwrap(),
            "--host",
            "h",
            "--identity",
            "ops",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Now ask a question. The reply is reached only by reading past every
    // one of those events.
    let r = c.call("status", json!({})).unwrap();
    assert!(r.is_object() || r.is_array(), "{r}");

    let mut kept = Vec::new();
    while let Some(e) = c.next_event() {
        kept.push(e);
    }
    assert!(
        kept.len() >= 3,
        "the apply's entries were read past and dropped: kept {}",
        kept.len()
    );
    assert!(
        kept.iter()
            .all(|e| e.pointer("/event/plan") == Some(&json!("p"))),
        "an event for a plan this identity does not subscribe to: {kept:?}"
    );
    // Drained is drained.
    assert_eq!(c.pending_events(), 0);
}

#[test]
fn a_plan_applies_through_a_registered_hook_and_every_verb_prints_its_line_last() {
    let d = TempDir::new("apply");
    let me = user_name(my_uid()).unwrap();
    fs::write(d.0.join("inventory.toml"), INVENTORY).unwrap();
    let site_file = d.0.join("plan.rue");
    fs::write(&site_file, format!("{}{PLAN}", site(&me, true))).unwrap();
    let daemon = Daemon::start(&d.0, &site_file, &[]);
    let hook = serve_hook(&daemon.socket, "act");

    // With no host reachable but through the hook, the plan applies.
    let out = rue(
        &daemon.socket,
        &[
            "apply",
            site_file.to_str().unwrap(),
            "--host",
            "h",
            "--identity",
            "ops",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let last = stdout.lines().last().unwrap_or("");
    assert!(
        last.starts_with("p.h.") && last.ends_with(": applied"),
        "{stdout}"
    );
    let id = last.split(':').next().unwrap().to_string();

    // status: one, and all
    let out = rue(&daemon.socket, &["status", &id, "--identity", "ops"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("applied (p on h)") && text.contains("wane at"),
        "{text}"
    );
    let out = rue(&daemon.socket, &["status", "--identity", "ops"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).lines().count(), 1);

    // the sole-identity user path: --identity omitted and two identities
    // map to this user: R0503, exit 2
    let out = rue(&daemon.socket, &["status"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("R0503"));

    // a second apply of the same plan: R0101, exit 75
    let out = rue(
        &daemon.socket,
        &[
            "apply",
            site_file.to_str().unwrap(),
            "--host",
            "h",
            "--identity",
            "ops",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(75),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // renew outside the window: R0102, exit 2
    let out = rue(
        &daemon.socket,
        &["renew", &id, "--wane", "2h", "--identity", "ops"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("R0102"));

    // recant: closed cleanly, exit 0, its line last. An instance that
    // closes because something refused it is exit 1; one an operator
    // recants did what was asked (section 6.8).
    let out = rue(&daemon.socket, &["recant", &id, "--identity", "ops"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.trim_end().ends_with("closed (reverted)"), "{stdout}");

    // a rehearsal against the real daemon: exit 0, nothing run
    let out = rue(
        &daemon.socket,
        &[
            "apply",
            site_file.to_str().unwrap(),
            "--host",
            "h",
            "--identity",
            "ops",
            "--dry-run",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("rehearsal: no reservation"), "{stdout}");

    // abandon by a non-admin: R0506, exit 2; the owner identity is not admin
    let out = rue(
        &daemon.socket,
        &["abandon", &id, "--reason", "x", "--identity", "owner"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("R0506"));

    // the journal file sink received the chain, and it verifies
    let out = Command::new(rue_bin())
        .args(["journal", "verify"])
        .arg(d.0.join("journal.ndjson"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stderr = daemon.stop();
    let served = hook.join().unwrap();
    // the do, then the undo, both through the hook; the rehearsal ran nothing
    let runs: Vec<&Value> = served.iter().filter(|r| r["op"] == "run").collect();
    assert_eq!(runs.len(), 2, "{served:?}\n{stderr}");
    assert!(stderr.contains("serving"), "{stderr}");
}

#[test]
fn a_spawned_child_registers_over_stdio_as_the_socket_owner() {
    let d = TempDir::new("spawn");
    let me = user_name(my_uid()).unwrap();
    fs::write(d.0.join("inventory.toml"), INVENTORY).unwrap();
    let site_file = d.0.join("plan.rue");
    fs::write(&site_file, format!("{}{PLAN}", site(&me, true))).unwrap();
    let stub = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stub-hook.sh");
    let spawn = format!("act=sh {} act", stub.display());
    let daemon = Daemon::start(&d.0, &site_file, &["--spawn", &spawn]);
    let mut c = Client::connect(&daemon.socket).unwrap();
    c.hello(Some("ops")).unwrap();
    assert_eq!(c.call("hooks", json!({})).unwrap(), json!(["act"]));
    let out = rue(
        &daemon.socket,
        &[
            "apply",
            site_file.to_str().unwrap(),
            "--host",
            "h",
            "--identity",
            "ops",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // A child registering a name outside may_register refuses to start.
    daemon.stop();
    let spawn = format!("other=sh {} other", stub.display());
    let mut c = Command::new(rued_bin());
    let out = c
        .arg("run")
        .arg("--site")
        .arg(&site_file)
        .arg("--store")
        .arg(d.0.join("store2"))
        .arg("--socket")
        .arg(d.0.join("s2.sock"))
        .arg("--group")
        .arg(my_gid().to_string())
        .arg("--spawn")
        .arg(&spawn)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("R0505"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn daemon_dry_run_mode_suspends_e0604_and_rehearses_everything() {
    let d = TempDir::new("dry");
    fs::write(d.0.join("inventory.toml"), INVENTORY).unwrap();
    let site_file = d.0.join("plan.rue");
    fs::write(&site_file, format!("{}{PLAN}", site("nobody", false))).unwrap();
    // Outside dry-run: E0604 refuses to start.
    let out = Command::new(rued_bin())
        .arg("run")
        .arg("--site")
        .arg(&site_file)
        .arg("--store")
        .arg(d.0.join("store0"))
        .arg("--socket")
        .arg(d.0.join("s0.sock"))
        .arg("--group")
        .arg(my_gid().to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains(&rue_core::diagnostics::Code::E0604.to_string()),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // In dry-run: every peer is the dry-run identity; every apply a rehearsal.
    let daemon = Daemon::start(&d.0, &site_file, &["--dry-run"]);
    let out = rue(
        &daemon.socket,
        &["apply", site_file.to_str().unwrap(), "--host", "h"],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("rehearsal"), "{stdout}");
    let mut c = Client::connect(&daemon.socket).unwrap();
    let h = c.hello(None).unwrap();
    assert!(h.dry_run && h.admin && h.identity == "dry-run");
    daemon.stop();
}

#[test]
fn a_group_that_does_not_exist_refuses_to_start() {
    let d = TempDir::new("group");
    fs::write(d.0.join("inventory.toml"), INVENTORY).unwrap();
    let site_file = d.0.join("plan.rue");
    fs::write(&site_file, format!("{}{PLAN}", site("x", true))).unwrap();
    let out = Command::new(rued_bin())
        .arg("run")
        .arg("--site")
        .arg(&site_file)
        .arg("--store")
        .arg(d.0.join("store"))
        .arg("--socket")
        .arg(d.0.join("s.sock"))
        .arg("--group")
        .arg("no-such-group-rue-test")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not exist"));
    let _ = std::io::stderr().flush();
}

#[test]
fn always_opens_every_gate_so_a_live_daemon_refuses_to_bind_it() {
    let d = TempDir::new("always");
    fs::write(d.0.join("inventory.toml"), INVENTORY).unwrap();
    let me = user_name(my_uid()).unwrap();
    let site_file = d.0.join("plan.rue");
    // The same site with `approval via: always()`.
    let text = site(&me, true).replace(
        "  execute via: hook(:act, transport: :api)\n",
        "  execute via: hook(:act, transport: :api)\n  approval via: always()\n",
    );
    fs::write(&site_file, format!("{text}{PLAN}")).unwrap();
    let run = |extra: &[&str]| {
        let mut c = Command::new(rued_bin());
        c.arg("run")
            .arg("--site")
            .arg(&site_file)
            .arg("--store")
            .arg(d.0.join("store"))
            .arg("--socket")
            .arg(d.0.join("s.sock"))
            .arg("--group")
            .arg(my_gid().to_string())
            .args(extra);
        c
    };
    // Live: refused, and the reason names the flag that would admit it.
    let out = run(&[]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        err.contains("always()") && err.contains("--dry-run"),
        "{err}"
    );
    // Dry-run: admitted, and the daemon serves.
    let daemon = Daemon::start(&d.0, &site_file, &["--dry-run"]);
    daemon.stop();
    let _ = std::io::stderr().flush();
}
