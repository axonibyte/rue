//! T3's shape against a real firewall on a real host (tier 5, task 14):
//! the plan opens a port by a fenced region in the host's packet-filter
//! file, reloads it, and commits when confirmed; a recant strips the
//! region and leaves the file as it was; drift under `:defer` holds the
//! instance until it is forced, and under `:clobber` is restored; a write
//! outside the step's footprint is refused; and the journal a hand has
//! edited fails to verify.
//!
//! The rules are scoped to the loopback alias the target is addressed by,
//! so a plan that severs ssh severs only itself and never the transport
//! reaper is watching over.

use rue_e2e::{
    firewall_file, firewall_reload, instance_of, last_line, must, require_provisioned_host, rue,
    target_read, target_write, Daemon, Site,
};

/// The plan under test: a region in the firewall file, a reload, and a
/// commit once confirmed. Its shape is T3's; its text is the harness's,
/// because a tenant's own site block names hooks this guest does not run.
fn plans() -> String {
    format!(
        r##"
defop :open_port, _ do
  footprint region: file("{file}", anchor: "rue-e2e")
  do: [region_set(file("{file}", anchor: "rue-e2e"), content: "# rue e2e: port 8443"), run("{reload}")]
  undo: :restore
  undo_locus: :target
end

defplan :open_mgmt_port, _ do
  backstop trigger: [unless_confirmed: 10m], locus: :target, arm_before: 1
  open_port()
  confirm()
  commit()
end
"##,
        file = firewall_file(),
        reload = firewall_reload()
    )
}

fn before() -> String {
    target_read(firewall_file())
}

#[test]
fn the_plan_opens_the_port_by_a_region_and_commits_when_confirmed() {
    require_provisioned_host();
    let was = before();
    let site = Site::new("firewall-commit", &plans());
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    let line = must("apply", &out);
    assert!(line.contains("committed"), "{line}");
    // The region is in the file, and the rest of it is untouched.
    let now = target_read(firewall_file());
    assert!(now.contains("# rue-region rue-e2e begin"), "{now}");
    assert!(now.contains("port 8443"), "{now}");
    for line in was.lines() {
        assert!(now.contains(line), "the file kept its own line {line:?}");
    }
    d.stop();
    // A committed plan leaves the region: that is what committing means.
    let after = target_read(firewall_file());
    assert!(after.contains("# rue-region rue-e2e begin"), "{after}");
}

#[test]
fn a_recant_strips_the_region_and_leaves_the_file_as_it_was() {
    require_provisioned_host();
    // A temporary plan of the same shape: it puts the region in, and the
    // recant takes it out. (The permanent one above cannot be recanted
    // once it commits, which is what committing means.)
    let temporary = format!(
        r##"
defop :open_port, _ do
  footprint region: file("{file}", anchor: "rue-e2e")
  do: [region_set(file("{file}", anchor: "rue-e2e"), content: "# rue e2e: port 8443"), run("{reload}")]
  undo: :restore
  undo_locus: :target
end

defplan :open_mgmt_port, _ do
  wane 1h, renew_within: 10m
  backstop trigger: [after: 1h], locus: :target, arm_before: 1
  open_port()
end
"##,
        file = firewall_file(),
        reload = firewall_reload()
    );
    let site = Site::new("firewall-recant", &temporary);
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    must("apply", &out);
    let id = last_line(&out)
        .split(':')
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(
        !id.is_empty(),
        "the line names the instance: {}",
        last_line(&out)
    );
    let was = target_read(firewall_file());
    assert!(was.contains("rue-region rue-e2e"), "{was}");
    let out = rue(&d.socket, &["recant", &id]);
    must("recant", &out);
    let now = target_read(firewall_file());
    assert!(
        !now.contains("rue-region rue-e2e"),
        "the region is gone: {now}"
    );
    d.stop();
}

#[test]
fn a_hand_edited_fact_under_defer_holds_the_instance_until_it_is_forced() {
    require_provisioned_host();
    // A `modified` fact, edited on the target behind the engine's back.
    let kept = "/etc/rue-e2e-kept";
    target_write(kept, "kept\n");
    let plans = format!(
        r##"
defop :touch_kept, _ do
  footprint modified: file("{kept}")
  drift :defer
  do: write(file("{kept}"), content: "changed\n")
  undo: :restore
  undo_locus: :target
end

defplan :keep, _ do
  wane 1h, renew_within: 10m
  touch_kept()
end
"##
    );
    let site = Site::new("firewall-drift", &plans);
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    let id = instance_of(&must("apply", &out));
    assert_eq!(target_read(kept), "changed\n");
    // A stranger edits it.
    target_write(kept, "a stranger was here\n");
    let out = rue(&d.socket, &["recant", &id]);
    assert_eq!(
        out.status.code(),
        Some(8),
        "drift-held is exit 8: {}",
        last_line(&out)
    );
    assert!(last_line(&out).contains("R0202"), "{}", last_line(&out));
    assert_eq!(
        target_read(kept),
        "a stranger was here\n",
        "the undo left the stranger's edit alone"
    );
    // Forced, the undo restores what the step found.
    let out = rue(&d.socket, &["recant", &id, "--force=drift"]);
    must("recant --force=drift", &out);
    assert_eq!(target_read(kept), "kept\n", "restored from the snapshot");
    d.stop();
}

#[test]
fn a_write_outside_the_step_s_footprint_is_refused_and_the_plan_reverts() {
    require_provisioned_host();
    let mine = "/etc/rue-e2e-mine";
    let theirs = "/etc/rue-e2e-theirs";
    target_write(theirs, "theirs\n");
    // The step's `do` writes a fact the plan declares elsewhere and this
    // step does not own: R0201.
    let plans = format!(
        r##"
defop :own_mine, _ do
  footprint owned: file("{mine}")
  do: write(file("{mine}"), content: "mine\n")
  undo: :restore
  undo_locus: :target
end

defop :overreach, _ do
  footprint owned: file("{theirs}")
  do: run("printf 'clobbered\n' > {mine}")
  undo: :restore
  undo_locus: :target
end

defplan :overreaching, _ do
  wane 1h, renew_within: 10m
  own_mine()
  overreach()
end
"##
    );
    let site = Site::new("firewall-footprint", &plans);
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    // The apply ends in a refusal: exit 1 with the verdict line, not a
    // diagnostic, because the plan checked and the world refused it.
    let line = last_line(&out);
    assert!(
        line.contains("closed"),
        "the plan reverted: {line}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let journal = std::fs::read_to_string(site.dir.join("journal.ndjson")).unwrap_or_default();
    assert!(
        journal.contains("footprint_violation"),
        "R0201 is journaled: {journal}"
    );
    // Both steps are undone: neither file is left behind.
    assert_eq!(target_read(mine), "", "{mine} is gone");
    d.stop();
}

#[test]
fn a_journal_a_hand_has_edited_fails_to_verify() {
    require_provisioned_host();
    let site = Site::new("firewall-journal", &plans());
    let d = Daemon::start(&site);
    let out = rue(
        &d.socket,
        &["apply", site.file.to_str().unwrap(), "--host", "fw-01"],
    );
    must("apply", &out);
    d.stop();
    let path = site.dir.join("journal.ndjson");
    let text = std::fs::read_to_string(&path).expect("the journal");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() > 3, "the journal has entries: {}", lines.len());
    // Verified as it stands.
    let out = std::process::Command::new(rue_e2e::bin("rue"))
        .arg("journal")
        .arg("verify")
        .arg(&path)
        .output()
        .expect("rue");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // One entry deleted: the chain breaks and says where.
    let cut: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .map(|(_, l)| *l)
        .collect();
    std::fs::write(&path, cut.join("\n") + "\n").expect("the edited journal");
    let out = std::process::Command::new(rue_e2e::bin("rue"))
        .arg("journal")
        .arg("verify")
        .arg(&path)
        .output()
        .expect("rue");
    assert!(!out.status.success(), "a deleted entry is caught");
    let said =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(
        said.contains("seq") || said.contains("chain") || said.contains("hash"),
        "the failure names where: {said}"
    );
}
