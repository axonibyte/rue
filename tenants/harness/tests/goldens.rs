//! Tier 2: every declared artifact -- the IR, the verdicts, the listings and
//! the artifacts the texts produce, the front end's diagnostics for the
//! texts it refuses, and the state table -- matches its expected file byte
//! for byte; every file under an expected directory is an artifact; and the
//! writer refuses to write unless told to.

use std::fs;
use std::path::{Path, PathBuf};

use rue_tenants::golden::{actual_path, compare_bytes, render_mismatch, repo_root};
use rue_tenants::{artifacts, STATE_TABLE};

#[test]
fn every_artifact_matches_its_expected_file() {
    let root = repo_root().unwrap();
    let arts = artifacts();
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
fn no_orphan_expected_files_and_no_missing_artifacts() {
    let root = repo_root().unwrap();
    let found = expected_files(&root);
    let mut declared: Vec<String> = artifacts().into_iter().map(|a| a.path).collect();
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

#[test]
fn the_writer_refuses_without_the_variable_and_touches_nothing() {
    let root = repo_root().unwrap();
    let before: Vec<(PathBuf, std::time::SystemTime)> = artifacts()
        .iter()
        .map(|a| root.join(&a.path))
        .map(|p| {
            let t = fs::metadata(&p).unwrap().modified().unwrap();
            (p, t)
        })
        .collect();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rue-goldens"))
        .env_remove("RUE_UPDATE_GOLDENS")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("RUE_UPDATE_GOLDENS=1"));
    for (p, t) in before {
        assert_eq!(
            fs::metadata(&p).unwrap().modified().unwrap(),
            t,
            "{} was touched",
            p.display()
        );
    }
}
