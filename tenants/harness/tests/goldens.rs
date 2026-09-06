//! Tier 2: every declared artifact matches its expected file byte for byte,
//! and every file under an expected directory is either an input a case
//! declares or an artifact the crates reproduce.

use std::fs;
use std::path::{Path, PathBuf};

use rue_tenants::golden::{actual_path, compare_bytes, render_mismatch, repo_root};
use rue_tenants::{artifacts, inputs, STATE_TABLE};

#[test]
fn every_artifact_matches_its_expected_file() {
    let root = repo_root().unwrap();
    let arts = artifacts(&root);
    assert!(!arts.is_empty());
    let mut failures = Vec::new();
    for a in &arts {
        let expected_file = root.join(&a.path);
        let bytes = match &a.bytes {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("could not produce {}: {e}", a.path));
                continue;
            }
        };
        let expected = match fs::read(&expected_file) {
            Ok(e) => e,
            Err(e) => {
                failures.push(format!(
                    "missing expected file {} ({e})",
                    expected_file.display()
                ));
                continue;
            }
        };
        if let Some(m) = compare_bytes(&expected, bytes) {
            let actual = actual_path(&root, &a.path);
            fs::create_dir_all(actual.parent().unwrap()).unwrap();
            fs::write(&actual, bytes).unwrap();
            failures.push(render_mismatch(&expected_file, &actual, &m));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} artifacts differ:\n{}",
        failures.len(),
        arts.len(),
        failures.join("\n")
    );
}

/// Every file under an `expected` directory beneath tenants/, plus the
/// generated transition table, as `/`-separated paths relative to the root.
fn expected_files(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.push(p);
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(&root.join("tenants"), &mut files);
    let mut out: Vec<String> = files
        .into_iter()
        .filter(|p| {
            p.strip_prefix(root)
                .ok()
                .and_then(|r| r.parent())
                .is_some_and(|parent| parent.components().any(|c| c.as_os_str() == "expected"))
        })
        .map(|p| {
            p.strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    if root.join(STATE_TABLE).is_file() {
        out.push(STATE_TABLE.to_string());
    }
    out.sort();
    out
}

#[test]
fn no_orphan_expected_files_and_no_missing_inputs() {
    let root = repo_root().unwrap();
    let found = expected_files(&root);
    let mut declared: Vec<String> = artifacts(&root)
        .into_iter()
        .map(|a| a.path)
        .chain(inputs())
        .collect();
    declared.sort();
    declared.dedup();
    let orphans: Vec<&String> = found.iter().filter(|f| !declared.contains(f)).collect();
    let missing: Vec<&String> = declared.iter().filter(|d| !found.contains(d)).collect();
    assert!(
        orphans.is_empty(),
        "expected files nothing declares: {orphans:?}"
    );
    assert!(
        missing.is_empty(),
        "declared files that do not exist: {missing:?}"
    );
}
