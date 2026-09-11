//! The resolver's own diagnostics (docs/ROADMAP.md 6.7, the E01xx the
//! front end raises in unit B): each on a small file written to a
//! temporary directory beside the inventory it needs.

use std::fs;
use std::path::PathBuf;

use rue_core::diagnostics::Code;
use rue_surface::resolve::{resolve, Options};

const INVENTORY: &str = r#"
[[host]]
name = "db-01"
address = "10.0.0.1"
os = "freebsd"
roles = ["db"]
reach = ["ssh"]
filesystem = true
scheduler = "cron"

[[host]]
name = "api-01"
address = "10.0.0.2"
os = "appliance"
reach = ["api"]
filesystem = false

[authenticators]
oncall = { human = true }
"#;

const SITE: &str = r#"site do
  inventory from: file("inventory.toml")
  journal to: local()
  operators do
    identity :requester, user: "ops", operator_for: :all, admin: true
  end
end
"#;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("rue-resolve-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("inventory.toml"), INVENTORY).unwrap();
        Dir(d)
    }
    fn file(&self, name: &str, body: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, format!("rue 0\n{SITE}\n{body}")).unwrap();
        p
    }
    fn raw(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, text).unwrap();
        p
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // A directory the test could not remove is a leak the test caused, not
        // an error to discard (see engine/tests/common/mod.rs).
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if !std::thread::panicking() => {
                panic!("{} was not removed: {e}", self.0.display())
            }
            Err(_) => {}
        }
    }
}

fn codes(path: &std::path::Path, host: &str) -> Vec<(Code, String)> {
    let opts = Options {
        suspend_e0604: false,
        host: Some(host.into()),
        plan: None,
        requester: None,
        inventory: None,
    };
    match resolve(path, &opts) {
        Ok(_) => Vec::new(),
        Err(ds) => ds.into_iter().map(|d| (d.code, d.render())).collect(),
    }
}

const POSTURE: &str = r#"defop :posture, _ do
  footprint owned: file("/etc/a")
  do: write(file("/etc/a"), content: "x")
  undo: :restore
end
"#;

#[test]
fn a_clean_file_resolves() {
    let d = Dir::new("clean");
    let f = d.file(
        "plan.rue",
        &format!("{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"),
    );
    assert!(codes(&f, "db-01").is_empty());
}

#[test]
fn an_unknown_name_is_e0102_with_the_nearest_suggestion() {
    let d = Dir::new("e0102");
    let f = d.file(
        "plan.rue",
        &format!("{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  postrue()\nend\n"),
    );
    let cs = codes(&f, "db-01");
    assert_eq!(cs.len(), 1, "{cs:?}");
    assert_eq!(cs[0].0, Code::E0102);
    assert!(cs[0].1.contains("did you mean posture"), "{}", cs[0].1);
    // An unknown probe, and an unknown primitive.
    let f = d.file("plan2.rue", &format!("{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  observe reach() as r\n  posture()\nend\n"));
    assert!(
        matches!(codes(&f, "db-01").as_slice(), [(Code::E0102, m)] if m.contains("unknown probe"))
    );
    let f = d.file("plan3.rue", "defop :o, _ do\n  footprint owned: file(\"/etc/a\")\n  do: wrte(file(\"/etc/a\"), content: \"x\")\n  undo: :restore\nend\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n  o()\nend\n");
    assert!(
        matches!(codes(&f, "db-01").as_slice(), [(Code::E0102, m)] if m.contains("did you mean write"))
    );
}

#[test]
fn two_clauses_with_one_pattern_are_e0103() {
    let d = Dir::new("e0103");
    let f = d.file("plan.rue", &format!("{POSTURE}\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(cs.iter().any(|(c, _)| *c == Code::E0103), "{cs:?}");
}

#[test]
fn an_import_cycle_is_e0104_and_a_missing_import_too() {
    let d = Dir::new("e0104");
    d.raw("a.rue", "rue 0\nimport \"b.rue\" as b\n");
    let a = d.raw("a.rue", "rue 0\nimport \"b.rue\" as b\n");
    d.raw("b.rue", "rue 0\nimport \"a.rue\" as a\n");
    let cs = codes(&a, "db-01");
    assert!(
        cs.iter()
            .any(|(c, m)| *c == Code::E0104 && m.contains("cycle")),
        "{cs:?}"
    );
    let f = d.raw("c.rue", "rue 0\nimport \"nope.rue\" as n\n");
    let cs = codes(&f, "db-01");
    assert!(
        cs.iter()
            .any(|(c, m)| *c == Code::E0104 && m.contains("cannot read")),
        "{cs:?}"
    );
}

#[test]
fn an_unbounded_repeat_is_e0106_and_a_repeated_member_is_e0113() {
    let d = Dir::new("e0106");
    let f = d.file("plan.rue", &format!("{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  repeat over: guests, as g do\n    posture()\n  end\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(
        matches!(cs.as_slice(), [(Code::E0106, m)] if m.contains("unbounded")),
        "{cs:?}"
    );
    let f = d.file("plan2.rue", &format!("{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  repeat over: [:a, :b, :a], as g, max: 3 do\n    posture()\n  end\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(matches!(cs.as_slice(), [(Code::E0113, _)]), "{cs:?}");
}

#[test]
fn a_comparison_against_unknown_is_e0108() {
    let d = Dir::new("e0108");
    let f = d.file("plan.rue", &format!("{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  assert peer == :unknown\n  posture()\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(matches!(cs.as_slice(), [(Code::E0108, _)]), "{cs:?}");
}

#[test]
fn an_output_read_before_its_step_is_e0110() {
    let d = Dir::new("e0110");
    let f = d.file("plan.rue", "defop :o, _ do\n  footprint owned: file(\"/etc/a\")\n  do: write(file(\"/etc/a\"), content: pw)\n  undo: :restore\nend\ndefop :issue, _ do\n  footprint modified: api.token\n  do: hook(:issue)\n  undo: hook(:revoke, idempotent: true)\n  undo_pre api.token\n  outputs pw, secret: true\n  locus: :controller\nend\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n  o(pw: tok.pw)\n  issue() as tok\nend\n");
    let cs = codes(&f, "db-01");
    assert!(
        matches!(cs.as_slice(), [(Code::E0110, m)] if m.contains("tok.pw")),
        "{cs:?}"
    );
}

#[test]
fn a_clause_on_a_non_contract_fact_is_e0111_and_no_matching_clause_is_e0112() {
    let d = Dir::new("e0111");
    let f = d.file("plan.rue", "defop :o, %{load: :high} do\n  footprint owned: file(\"/etc/a\")\n  do: write(file(\"/etc/a\"), content: \"x\")\n  undo: :restore\nend\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n  o()\nend\n");
    let cs = codes(&f, "db-01");
    assert!(
        cs.iter()
            .any(|(c, m)| *c == Code::E0111 && m.contains("load")),
        "{cs:?}"
    );
    let f = d.file("plan2.rue", "defop :o, %{os: :windows} do\n  footprint owned: file(\"/etc/a\")\n  do: write(file(\"/etc/a\"), content: \"x\")\n  undo: :restore\nend\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n  o()\nend\n");
    let cs = codes(&f, "db-01");
    assert!(
        matches!(cs.as_slice(), [(Code::E0112, m)] if m.contains("db-01")),
        "{cs:?}"
    );
    // A list pattern matches any member; a static host locus dispatches on it.
    let f = d.file("plan3.rue", "defop :o, %{os: [:linux, :freebsd]} do\n  footprint owned: file(\"/etc/a\")\n  do: write(file(\"/etc/a\"), content: \"x\")\n  undo: :restore\nend\ndefop :b, %{os: :appliance} do\n  footprint modified: bmc.account(\"x\")\n  do: hook(:enable)\n  undo: hook(:disable, idempotent: true)\n  undo_pre bmc.account(\"x\")\n  locus: host(\"api-01\")\nend\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n  o()\n  b()\nend\n");
    assert!(codes(&f, "db-01").is_empty());
}

#[test]
fn the_version_marker_and_a_parse_error_reach_the_caller() {
    let d = Dir::new("parse");
    let f = d.raw(
        "plan.rue",
        "defplan :p, %{name: \"db-01\"} do\n  wane 1h\nend\n",
    );
    let cs = codes(&f, "db-01");
    assert!(cs.iter().any(|(c, _)| *c == Code::E0105), "{cs:?}");
    let f = d.raw(
        "plan2.rue",
        "rue 0\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n  posture(\nend\n",
    );
    let cs = codes(&f, "db-01");
    assert!(cs.iter().any(|(c, _)| *c == Code::E0101), "{cs:?}");
}

const SITE_FULL: &str = r#"site do
  inventory from: file("inventory.toml")
  journal to: local()
  approval via: hook(:authority)
  backstop scheduler: cron()
  operators do
    identity :requester, user: "ops", operator_for: :all, admin: true
  end
  hooks do
    registrar :authority, user: "approvald", may_register: [:authority]
  end
end
"#;

fn ir(path: &std::path::Path, host: &str) -> rue_core::ir::PlanIr {
    let opts = Options {
        suspend_e0604: false,
        host: Some(host.into()),
        plan: None,
        requester: None,
        inventory: None,
    };
    resolve(path, &opts).unwrap_or_else(|ds| {
        panic!(
            "{}",
            ds.iter().map(|d| d.render()).collect::<Vec<_>>().join("\n")
        )
    })
}

#[test]
fn roles_fill_slots_by_priority_then_name_and_an_unfilled_slot_stays() {
    let d = Dir::new("roles");
    let f = d.raw("plan.rue", &format!("rue 0\n{SITE}\n{POSTURE}\ndefop :snapshot, _ do\n  footprint modified: db.snapshot\n  do: hook(:snap)\n  undo: hook(:unsnap, idempotent: true)\n  undo_pre db.snapshot\n  locus: :controller\nend\ndefop :fence, _ do\n  footprint\n  do: hook(:fence)\n  undo_locus: :none\n  refusal: knell, guard: off, cost: :none, reason: \"x\", ack: :none, reason: \"y\"\nend\ndefrole :db do\n  :before 50 snapshot()\n  :before knell fence()\nend\ndefrole :primary do\n  :before 50 posture()\nend\ndefrole :web do\n  :before posture()\nend\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  slot :before\n  slot :after\n  posture()\nend\n"));
    let plan = ir(&f, "db-01").plan;
    let ids: Vec<String> = plan
        .body
        .iter()
        .map(|it| match it {
            rue_core::model::Item::Step(s) => s.op.id.clone(),
            rue_core::model::Item::Knell(s) => format!("knell {}", s.op.id),
            rue_core::model::Item::Slot { name } => format!("slot {name}"),
            other => format!("{other:?}"),
        })
        .collect();
    // db-01 has roles db only: priority 50 snapshot, then the default-100 knell;
    // primary's contribution is not for this host; :after has none.
    assert_eq!(
        ids,
        vec!["snapshot", "knell fence", "slot after", "posture"]
    );
}

#[test]
fn a_protocol_expands_to_the_role_impl_or_its_default_and_needs_its_inverse() {
    let d = Dir::new("protocol");
    let body = format!("{POSTURE}\ndefop :pg_quiesce, _ do\n  footprint modified: pg.state\n  do: hook(:pg)\n  undo: hook(:pg_resume, idempotent: true)\n  undo_pre pg.state\n  locus: :controller\nend\ndefop :pg_resume, _ do\n  footprint modified: pg.state\n  do: hook(:pg_resume)\n  undo: hook(:pg, idempotent: true)\n  undo_pre pg.state\n  locus: :controller\nend\n");
    let f = d.raw("plan.rue", &format!("rue 0\n{SITE}\n{body}defprotocol :quiesce, inverse: :resume do\n  default posture()\nend\ndefprotocol :resume do\n  default posture()\nend\ndefimpl :quiesce, for: :db do\n  pg_quiesce()\n  posture()\nend\ndefimpl :resume, for: :db do\n  pg_resume()\nend\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  quiesce()\nend\n"));
    let plan = ir(&f, "db-01").plan;
    let ids: Vec<&str> = plan
        .body
        .iter()
        .filter_map(|it| match it {
            rue_core::model::Item::Step(s) => Some(s.op.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids,
        vec!["pg_quiesce", "posture"],
        "the db impl, spliced in place"
    );
    // No impl for the host's roles: the default.
    let f = d.raw("plan2.rue", &format!("rue 0\n{SITE}\n{body}defprotocol :quiesce do\n  default posture()\nend\ndefimpl :quiesce, for: :web do\n  pg_quiesce()\nend\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  quiesce()\nend\n"));
    let plan = ir(&f, "db-01").plan;
    let ids: Vec<&str> = plan
        .body
        .iter()
        .filter_map(|it| match it {
            rue_core::model::Item::Step(s) => Some(s.op.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec!["posture"]);
    // An impl without its paired inverse is E0103.
    let f = d.raw("plan3.rue", &format!("rue 0\n{SITE}\n{body}defprotocol :quiesce, inverse: :resume do\n  default posture()\nend\ndefprotocol :resume do\n  default posture()\nend\ndefimpl :quiesce, for: :db do\n  pg_quiesce()\nend\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  quiesce()\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(
        matches!(cs.as_slice(), [(Code::E0103, m)] if m.contains("inverse")),
        "{cs:?}"
    );
}

#[test]
fn a_defprim_call_is_a_classed_run_template() {
    let d = Dir::new("defprim");
    let f = d.raw("plan.rue", &format!("rue 0\n{SITE}\ndefprim :svc, name: name, action: action do\n  run \"service #{{name}} #{{action}}\", classes: %{{name: :target_local, action: :controller}}\nend\ndefop :restart, _ do\n  footprint modified: svc.sshd\n  do: svc(name: \"sshd\", action: verb)\n  undo: svc(name: \"sshd\", action: \"start\")\n  undo_pre svc.sshd\nend\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  restart(verb: \"restart\")\nend\n"));
    let plan = ir(&f, "db-01").plan;
    let rue_core::model::Item::Step(s) = &plan.body[0] else {
        panic!()
    };
    let rue_core::body::Prim::Call(c) = &s.op.do_[0] else {
        panic!("{:?}", s.op.do_)
    };
    assert_eq!(c.prim, "svc");
    // `verb` is bound at the call to a literal, so the template carries the
    // literal. This once expected a parameter named `verb` for the request
    // to bind, which was the defect a_call_s_arguments_reach_the_body_as_
    // what_they_were_bound_to describes.
    assert_eq!(
        c.run,
        vec![
            rue_core::body::text("service "),
            rue_core::body::text("sshd"),
            rue_core::body::text(" "),
            rue_core::body::text("restart")
        ]
    );
    assert_eq!(
        c.args
            .iter()
            .map(|a| (a.name.as_str(), a.class))
            .collect::<Vec<_>>(),
        vec![
            ("action", rue_core::body::ArgClass::Controller),
            ("name", rue_core::body::ArgClass::TargetLocal)
        ]
    );
}

#[test]
fn a_kind_mismatch_is_e0107_and_when_arms_binding_an_alias_differently_is_e0114() {
    let d = Dir::new("e0107");
    let f = d.file("plan.rue", "defop :o, _, drift: :defer do\n  footprint owned: file(\"/etc/a\")\n  do: write(file(\"/etc/a\"), content: \"x\")\n  undo: :restore\nend\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n  o(drift: 5)\nend\n");
    let cs = codes(&f, "db-01");
    assert!(
        matches!(cs.as_slice(), [(Code::E0107, m)] if m.contains("expects atom, got int")),
        "{cs:?}"
    );
    let f = d.file(
        "plan2.rue",
        &format!("{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 4\n  posture()\nend\n"),
    );
    let cs = codes(&f, "db-01");
    assert!(matches!(cs.as_slice(), [(Code::E0107, _)]), "{cs:?}");
    let f = d.file("plan3.rue", "defop :a, _ do\n  footprint modified: api.a\n  do: hook(:a)\n  undo: hook(:ua, idempotent: true)\n  undo_pre api.a\n  outputs tok, secret: true\n  locus: :controller\nend\ndefop :b, _ do\n  footprint modified: api.b\n  do: hook(:b)\n  undo: hook(:ub, idempotent: true)\n  undo_pre api.b\n  outputs tok\n  locus: :controller\nend\ndefplan :p, %{name: \"db-01\"} do\n  wane 1h\n  when healthy do\n    a() as t\n  else\n    b() as t\n  end\nend\n");
    let cs = codes(&f, "db-01");
    assert!(matches!(cs.as_slice(), [(Code::E0114, _)]), "{cs:?}");
}

#[test]
fn the_site_is_validated() {
    let d = Dir::new("site");
    // A binding kind the line does not admit.
    let f = d.raw("plan.rue", &format!("rue 0\nsite do\n  inventory from: ldap(\"x\")\n  journal to: local()\n  operators do\n    identity :requester, user: \"ops\", operator_for: :all, admin: true\n  end\nend\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(
        cs.iter()
            .any(|(c, m)| *c == Code::E0601 && m.contains("ldap")),
        "{cs:?}"
    );
    // A contract violation: file() without a path; hold without until:.
    let f = d.raw("plan2.rue", &format!("rue 0\nsite do\n  inventory from: file(\"inventory.toml\")\n  journal to: file()\n  secrets deliver_to: [hold()]\n  operators do\n    identity :requester, user: \"ops\", operator_for: :all, admin: true\n  end\nend\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"));
    let cs = codes(&f, "db-01");
    assert_eq!(
        cs.iter().filter(|(c, _)| *c == Code::E0602).count(),
        2,
        "{cs:?}"
    );
    // A hook without its atom.
    let f = d.raw("plan2b.rue", &format!("rue 0\nsite do\n  inventory from: file(\"inventory.toml\")\n  journal to: local()\n  approval via: hook(\"authority\")\n  operators do\n    identity :requester, user: \"ops\", operator_for: :all, admin: true\n  end\n  hooks do\n    registrar :authority, user: \"approvald\", may_register: [:authority]\n  end\nend\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(
        matches!(cs.as_slice(), [(Code::E0602, m)] if m.contains("hook")),
        "{cs:?}"
    );
    // No journal; no operators; a hook no registrar may register.
    let f = d.raw("plan3.rue", &format!("rue 0\nsite do\n  inventory from: file(\"inventory.toml\")\nend\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(
        cs.iter().any(|(c, _)| *c == Code::E0603) && cs.iter().any(|(c, _)| *c == Code::E0604),
        "{cs:?}"
    );
    let f = d.raw("plan4.rue", &format!("rue 0\nsite do\n  inventory from: file(\"inventory.toml\")\n  journal to: local()\n  approval via: hook(:authority)\n  operators do\n    identity :requester, user: \"ops\", operator_for: :all, admin: true\n  end\nend\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"));
    let cs = codes(&f, "db-01");
    assert!(
        matches!(cs.as_slice(), [(Code::E0605, m)] if m.contains("authority")),
        "{cs:?}"
    );
    // The full block is clean.
    let f = d.raw("plan5.rue", &format!("rue 0\n{SITE_FULL}\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"));
    assert!(codes(&f, "db-01").is_empty());
}

// --- the site bindings a daemon reads --------------------------------------

#[test]
fn a_site_may_declare_more_than_one_journal_sink_and_keeps_all_of_them() {
    use rue_surface::resolve::site_bindings;
    let d = Dir::new("site-two-sinks");
    // 5.10 delivers to every declared sink and all must acknowledge, so a
    // second sink is redundancy a site asked for and is entitled to. While
    // this was one binding the resolver kept the first and dropped the
    // rest without a word, which turned a two-sink site into a one-sink
    // site that still looked right in its own text.
    let text = r#"rue 0
site do
  inventory from: file("inventory.toml")
  journal to: file("journal.ndjson"), hook(:host_log), stdout(), sign: key("keys/journal")
  execute via: local()
  operators do
    identity :ops, user: "ops", operator_for: :all, admin: true
  end
  hooks do
    registrar :ops, user: "ops", may_register: [:host_log]
  end
end
"#;
    let f = d.raw("site.rue", text);
    let sb = site_bindings(&f).unwrap();
    let kinds: Vec<&str> = sb.decl.journal.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(kinds, vec!["file", "hook", "stdout"], "in declared order");
    assert_eq!(sb.decl.journal[0].arg.as_deref(), Some("journal.ndjson"));
    assert_eq!(sb.decl.journal[1].arg.as_deref(), Some("host_log"));
    // The signing key rides in the same declaration and is not a sink.
    assert_eq!(
        sb.decl.journal_sign.as_ref().map(|b| b.kind.as_str()),
        Some("key")
    );
}

#[test]
fn site_bindings_carry_operators_registrars_and_the_inventory_and_refuse_an_identity_with_no_user()
{
    use rue_surface::resolve::site_bindings;
    let d = Dir::new("site-bindings");
    let text = r#"rue 0
site do
  inventory from: file("inventory.toml")
  journal to: file("journal.ndjson")
  execute via: [ssh(identity: "keys/id_ed25519", known_hosts: "known_hosts", user: "root"), hook(:actuate, transport: :api)]
  max_wait 20m
  operators do
    identity :ops, user: "ops", operator_for: :all, admin: true, subscribe: [:p]
    identity :host, user: :socket_owner, operator_for: [:p, :q]
  end
  hooks do
    registrar :host, user: :socket_owner, may_register: [:actuate]
  end
end
"#;
    let f = d.raw("site.rue", text);
    let sb = site_bindings(&f).unwrap();
    assert_eq!(sb.dir, d.0);
    assert_eq!(sb.inventory.hosts.len(), 2);
    assert_eq!(sb.inventory.contracts[0].address, "10.0.0.1");
    assert_eq!(sb.decl.journal.len(), 1);
    assert_eq!(sb.decl.journal[0].kind, "file");
    assert_eq!(sb.decl.journal[0].arg.as_deref(), Some("journal.ndjson"));
    assert_eq!(sb.decl.execute.len(), 2);
    assert_eq!(sb.decl.max_wait.map(|d| d.seconds), Some(1200));
    let ops = &sb.decl.identities;
    assert_eq!(ops.len(), 2);
    assert_eq!(
        (
            ops[0].name.as_str(),
            ops[0].user.as_deref(),
            ops[0].admin,
            &ops[0].subscribe
        ),
        ("ops", Some("ops"), true, &vec!["p".to_string()])
    );
    assert!(ops[0].admits_plan("anything"));
    assert_eq!(
        (ops[1].name.as_str(), ops[1].user.as_deref(), ops[1].admin),
        ("host", Some("socket_owner"), false)
    );
    assert!(ops[1].admits_plan("p") && ops[1].admits_plan("q") && !ops[1].admits_plan("r"));
    let regs = &sb.decl.registrars;
    assert_eq!(regs[0].user.as_deref(), Some("socket_owner"));
    assert_eq!(regs[0].may_register, vec!["actuate".to_string()]);

    // An identity or a registrar with no OS user is E0602.
    let bad = d.raw(
        "bad.rue",
        &text.replace("identity :ops, user: \"ops\", ", "identity :ops, "),
    );
    let diags = site_bindings(&bad).unwrap_err();
    assert!(
        diags
            .iter()
            .any(|x| x.code == Code::E0602 && x.message.contains("identity :ops names no OS user")),
        "{diags:?}"
    );
    let bad = d.raw(
        "bad2.rue",
        &text.replace(
            "registrar :host, user: :socket_owner, ",
            "registrar :host, ",
        ),
    );
    let diags = site_bindings(&bad).unwrap_err();
    assert!(
        diags.iter().any(
            |x| x.code == Code::E0602 && x.message.contains("registrar :host names no OS user")
        ),
        "{diags:?}"
    );
    // A missing inventory file is E0602 at the binding.
    let bad = d.raw("bad3.rue", &text.replace("inventory.toml", "nowhere.toml"));
    let diags = site_bindings(&bad).unwrap_err();
    assert!(
        diags
            .iter()
            .any(|x| x.code == Code::E0602 && x.message.contains("nowhere.toml")),
        "{diags:?}"
    );
}

// --- probes across files ---------------------------------------------------

/// The IR a file resolves to, for the one plan it has.
fn ir_of(path: &std::path::Path, host: &str) -> rue_core::ir::PlanIr {
    let opts = Options {
        suspend_e0604: false,
        host: Some(host.into()),
        plan: None,
        requester: None,
        inventory: None,
    };
    resolve(path, &opts)
        .unwrap_or_else(|ds| panic!("{:?}", ds.iter().map(|d| d.render()).collect::<Vec<_>>()))
}

#[test]
fn an_imported_probe_is_declared_by_the_name_every_reference_uses() {
    // The engine finds a probe by its name and nothing else, so the IR's
    // declaration and every reference to it must agree. They did not: an
    // imported probe was declared `lib.ready` while an `observe lib.ready()`
    // in the importer referred to `ready`, and a guard inside an op imported
    // from that file named `ready` too. Checked clean; unrunnable, since the
    // engine found no declaration and sent the host an empty body.
    let d = Dir::new("probe-names");
    d.raw(
        "lib.rue",
        "rue 0\ndefprobe :ready do\n  run \"true\"\n  locus :target\nend\n\
         defop :guarded, _ do\n  footprint owned: file(\"/etc/g\")\n  do: write(file(\"/etc/g\"), content: \"x\")\n  undo: :restore\n  post ready\nend\n",
    );
    let f = d.file(
        "plan.rue",
        "import \"lib.rue\" as lib\n\
         defplan :p, %{name: \"db-01\"} do\n  wane 1h\n  lib.guarded()\n  observe lib.ready() as r\nend\n",
    );
    let ir = ir_of(&f, "db-01");
    let declared: Vec<&str> = ir.plan.probes.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(declared, vec!["ready"], "declared by its bare name");
    let observed = ir.plan.body.iter().find_map(|it| match it {
        rue_core::model::Item::Observe { probe, .. } => Some(probe.as_str()),
        _ => None,
    });
    assert_eq!(
        observed,
        Some("ready"),
        "the observe names what is declared"
    );
    let post: Vec<String> = ir
        .plan
        .body
        .iter()
        .find_map(|it| match it {
            rue_core::model::Item::Step(s) => {
                Some(s.op.post.iter().map(|g| g.name.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(
        post,
        vec!["ready".to_string()],
        "and so does the imported op's guard"
    );
}

#[test]
fn an_open_import_s_probe_is_declared_too() {
    // An import with no alias puts its names in scope bare. Its probes were
    // never declared at all, so a guard naming one resolved and then named
    // nothing the engine could find.
    let d = Dir::new("probe-open");
    d.raw(
        "lib.rue",
        "rue 0\ndefprobe :ready do\n  run \"true\"\n  locus :target\nend\n",
    );
    let f = d.file(
        "plan.rue",
        &format!("import \"lib.rue\"\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  assert ready\n  posture()\nend\n"),
    );
    let ir = ir_of(&f, "db-01");
    assert!(
        ir.plan.probes.iter().any(|p| p.name == "ready"),
        "an open import's probe is not declared: {:?}",
        ir.plan.probes.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

#[test]
fn two_different_probes_with_one_bare_name_are_e0103() {
    // Bare names are what the engine resolves by, so two probes sharing one
    // would be indistinguishable: an imported op naming its own `ready`
    // would silently run the importer's. Refused rather than resolved by
    // precedence.
    let d = Dir::new("probe-clash");
    d.raw(
        "lib.rue",
        "rue 0\ndefprobe :ready do\n  run \"true\"\n  locus :target\nend\n",
    );
    let f = d.file(
        "plan.rue",
        &format!("import \"lib.rue\" as lib\ndefprobe :ready do\n  run \"false\"\n  locus :target\nend\n{POSTURE}\ndefplan :p, %{{name: \"db-01\"}} do\n  wane 1h\n  posture()\nend\n"),
    );
    let cs = codes(&f, "db-01");
    assert!(
        matches!(cs.as_slice(), [(Code::E0103, m)] if m.contains("ready")),
        "{cs:?}"
    );
}

// --- what a call binds -----------------------------------------------------

/// A value as text, each reference marked with where it resolves.
fn shown(v: &rue_core::body::Value) -> String {
    use rue_core::body::{Part, Ref, Value};
    let r = |r: &Ref| match r {
        Ref::Param(n) => format!("{{param {n}}}"),
        Ref::Controller(n) => format!("{{controller {n}}}"),
        Ref::Fact(n) => format!("{{fact {n}}}"),
        Ref::Secret(n) => format!("{{secret {n}}}"),
        Ref::Host(f) => format!("{{host {f}}}"),
        Ref::Output { step, name, .. } => format!("{{output {step}.{name}}}"),
    };
    match v {
        Value::Lit(s) => s.clone(),
        Value::Ref(x) => r(x),
        Value::Template(parts) => parts
            .iter()
            .map(|p| match p {
                Part::Lit(s) => s.clone(),
                Part::Ref(x) => r(x),
            })
            .collect(),
    }
}

/// The first step anywhere in a plan body, repeats included.
fn first_step(items: &[rue_core::model::Item]) -> Option<&rue_core::model::StepI> {
    use rue_core::model::Item;
    items.iter().find_map(|it| match it {
        Item::Step(s) | Item::Knell(s) => Some(s),
        Item::Repeat { body, .. } => first_step(body),
        _ => None,
    })
}

#[test]
fn a_call_s_arguments_reach_the_body_as_what_they_were_bound_to() {
    // 6.4: an op is a template expanded at check time, so the body uses what
    // the call bound. It did not: every argument reached the body as a
    // parameter named after the OP's parameter, for the request to bind. So
    // T1's `service_posture(posture: "PermitRootLogin yes")` wrote whatever a
    // request said, or refused, and never the text's literal; T2's
    // `record_succession(entry: succession_entry)` refused with "nothing
    // binds entry" however the request bound succession_entry; and a repeat
    // variable spelled differently from the op's parameter read a controller
    // value nothing held. Every tenant checked clean; T2 found it by running.
    let d = Dir::new("call-bindings");
    let f = d.file(
        "plan.rue",
        "defop :note, _ do\n  footprint append_only: file(\"/var/log/n\"), modified: slot.state(n), modified: slot.state(greeting)\n  \
         do: [append(file(\"/var/log/n\"), line: \"#{greeting} #{who} #{where} #{n}\"), run(\"echo #{greeting} #{count}\"), hook(:tally, as: who, mode: mode)]\n  \
         undo: compensate: append(file(\"/var/log/n\"), line: \"undone\")\n  undo_pre file(\"/var/log/n\")\nend\n\
         defplan :p, %{name: \"db-01\"} do\n  wane 1h\n  repeat over: items, as item, max: 4 do\n    \
         note(greeting: \"hello\", who: requester_name, where: host.address, n: item, count: 3, mode: :fast)\n  end\nend\n",
    );
    let plan = ir(&f, "db-01").plan;
    let s = first_step(&plan.body).expect("the repeat holds the step");
    use rue_core::body::Prim;
    let Prim::Append(a) = &s.op.do_[0] else {
        panic!("{:?}", s.op.do_)
    };
    assert_eq!(
        shown(&a.line),
        "hello {param requester_name} {host address} {controller item}",
        "a literal is substituted, a plan name is the request's parameter by \
         that name, a host field is the host's, a repeat variable is the \
         controller's by its own name"
    );
    let Prim::Run(r) = &s.op.do_[1] else {
        panic!("{:?}", s.op.do_)
    };
    assert_eq!(
        shown(&rue_core::body::Value::Template(r.cmd.clone())),
        "echo hello 3"
    );
    let Prim::Hook(h) = &s.op.do_[2] else {
        panic!("{:?}", s.op.do_)
    };
    let args: Vec<(String, String)> = h
        .args
        .iter()
        .map(|a| (a.name.clone(), shown(&a.value)))
        .collect();
    assert_eq!(
        args,
        vec![
            ("as".to_string(), "{param requester_name}".to_string()),
            ("mode".to_string(), ":fast".to_string()),
        ],
        "an atom is substituted as it would read written in place"
    );
    let shapes: Vec<&str> = s.op.footprint.iter().map(|e| e.shape.as_str()).collect();
    assert_eq!(
        shapes,
        vec!["file:/var/log/n", "slot:state:{item}", "slot:state:hello"],
        "a fact's instance names what the call bound, or is the literal"
    );
}
