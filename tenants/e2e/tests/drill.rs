//! Tier 5: a drill against a real canary (ROADMAP 7.14; `docs/issues/0003`).
//!
//! A drill is the difference between "the undo is written" and "the undo
//! ran last night and put the machine back". It applies a real plan to a
//! host the inventory declares a canary, recants it, and journals an
//! attestation naming every fact it touched with the digest before and
//! after. Here that happens over a real sshd against real files, and the
//! attestation is read back out of the chain by `rue journal verify
//! --attestations`, which is what the acceptance line asks for: journaled,
//! and verified.

use std::fs;

use rue_e2e::{bin, must, require_provisioned_host, rue, Daemon, Site};

/// The same guest, under a second name the inventory declares a canary.
/// A drill refuses every host without that role, so the fixture has to
/// give it one -- which is the point of the role.
const CANARY: &str = r#"[[host]]
name = "canary-01"
address = "127.0.0.2"
roles = ["canary"]
reach = ["ssh"]
filesystem = true
scheduler = "cron"
"#;

fn canary_host(os: &str, rue_root: &str) -> String {
    format!("{CANARY}os = \"{os}\"\nrue_root = \"{rue_root}\"\n")
}

fn plans(f: &str) -> String {
    format!(
        r##"
defop :own_it, _ do
  footprint owned: file("{f}")
  do: write(file("{f}"), content: "drilled\n")
  undo: :restore
  undo_locus: :target
end

defplan :rehearse_it, _ do
  wane 1h, renew_within: 10m
  own_it()
end
"##
    )
}

fn site(name: &str, f: &str) -> Site {
    Site::with(
        name,
        &plans(f),
        "",
        &canary_host(rue_e2e::os_family(), &rue_e2e::rue_root().to_string_lossy()),
        "",
    )
}

#[test]
fn a_drill_applies_to_the_canary_recants_and_attests_in_the_chain() {
    require_provisioned_host();
    let f = "/etc/rue-e2e-drill";
    let _ = fs::remove_file(f);
    let site = site("drill", f);
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["drill", site.file.to_str().unwrap(), "--host", "canary-01"],
    );
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let line = must("drill", &out);
    assert!(line.contains("drilled"), "{text}");
    assert!(
        text.contains("file:/etc/rue-e2e-drill before=") && text.contains(" after="),
        "the attestation names the fact and both reads: {text}"
    );
    // The canary is as it was: the drill undid what it did.
    assert!(
        !std::path::Path::new(f).exists(),
        "the drill left its own file behind"
    );
    d.stop();

    // Journaled and verified: the chain carries the attestation, and
    // reading it back is `rue journal verify --attestations`.
    let journal = site.dir.join("journal.ndjson");
    let out = std::process::Command::new(bin("rue"))
        .arg("journal")
        .arg("verify")
        .arg(&journal)
        .arg("--attestations")
        .output()
        .expect("rue");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("chain verified")
            && text.contains("drill rehearse_it.canary-01")
            && text.contains("restored")
            && text.contains("drills: 1, of which 0 attested nothing"),
        "{text}"
    );
}

#[test]
fn a_drill_is_refused_on_a_host_the_inventory_does_not_call_a_canary() {
    require_provisioned_host();
    let f = "/etc/rue-e2e-drill-refused";
    let _ = fs::remove_file(f);
    let site = site("drill-refused", f);
    let d = Daemon::start(&site);
    // fw-01 is the same machine by its other name, and is not a canary.
    let out = rue(
        &d.socket,
        &["drill", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    let said =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert_ne!(out.status.code(), Some(0), "{said}");
    assert!(
        said.contains("R0410") && said.contains("not a canary"),
        "{said}"
    );
    assert!(
        !std::path::Path::new(f).exists(),
        "the refused drill applied a step"
    );
    d.stop();
}
