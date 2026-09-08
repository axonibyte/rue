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
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn codes(path: &std::path::Path, host: &str) -> Vec<(Code, String)> {
    let opts = Options {
        host: Some(host.into()),
        plan: None,
        requester: None,
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
