//! `rued migrate`: a dry run reports and writes nothing; a real run writes
//! the schema and the record the next start journals; a newer schema is
//! refused with R0502 and exit 1; usage errors exit 2.

use std::path::PathBuf;
use std::process::Command;

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "rued-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rued(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rued"))
        .args(args)
        .output()
        .expect("rued")
}

#[test]
fn a_dry_run_reports_the_steps_and_writes_nothing_then_the_real_run_migrates() {
    let d = TempDir::new("migrate");
    let store = d.0.join("store");
    std::fs::create_dir_all(store.join("instances")).unwrap();
    let out = rued(&["migrate", "--store", store.to_str().unwrap(), "--dry-run"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("would migrate") && text.contains("from schema 0 to 2"),
        "{text}"
    );
    assert!(text.contains("write schema 2"), "{text}");
    assert!(!store.join("schema").exists());

    let out = rued(&["migrate", "--store", store.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("rued: migrated"));
    assert_eq!(
        std::fs::read_to_string(store.join("schema"))
            .unwrap()
            .trim(),
        "2"
    );
    assert!(store.join("migrated.json").exists());
    let out = rued(&["migrate", "--store", store.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("nothing to do"));
}

#[test]
fn a_newer_schema_is_r0502_and_exit_one_and_usage_is_exit_two() {
    let d = TempDir::new("migrate-newer");
    let store = d.0.join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("schema"), "7\n").unwrap();
    let out = rued(&["migrate", "--store", store.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("R0502") && err.contains("schema 7"), "{err}");
    assert_eq!(
        std::fs::read_to_string(store.join("schema"))
            .unwrap()
            .trim(),
        "7",
        "untouched"
    );

    let out = rued(&["migrate"]);
    assert_eq!(out.status.code(), Some(2));
    let out = rued(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("rued "));
}
