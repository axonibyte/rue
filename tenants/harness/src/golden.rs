//! Where goldens live and how they are compared: the Rust side of the
//! prototype's `Rue.Proto.Golden`. Read-only: the prototype's
//! `rue-proto-goldens` is the only writer.

use std::env;
use std::path::{Path, PathBuf};

/// One golden: a path relative to the repository root, `/`-separated, and
/// the bytes that must be there, or the reason they could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub path: String,
    pub bytes: Result<Vec<u8>, String>,
}

/// The repository root: `RUE_REPO_ROOT` if set, otherwise the nearest
/// ancestor of the working directory that contains `.reaper.toml`.
pub fn repo_root() -> Result<PathBuf, String> {
    if let Ok(r) = env::var("RUE_REPO_ROOT") {
        if !r.is_empty() {
            return Ok(PathBuf::from(r));
        }
    }
    let mut d = env::current_dir().map_err(|e| e.to_string())?;
    loop {
        if d.join(".reaper.toml").is_file() {
            return Ok(d);
        }
        if !d.pop() {
            return Err("repo_root: no .reaper.toml in any ancestor of the working directory, and RUE_REPO_ROOT is unset".into());
        }
    }
}

/// Where a failing comparison leaves the bytes it actually produced, so a
/// human can diff them against the expected file.
pub fn actual_path(root: &Path, artifact_path: &str) -> PathBuf {
    root.join("target")
        .join("golden-actual")
        .join(artifact_path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub line: usize,
    pub expected_context: Vec<String>,
    pub actual_context: Vec<String>,
    pub expected_length: usize,
    pub actual_length: usize,
}

/// `None` when equal; otherwise the first differing line with context.
pub fn compare_bytes(expected: &[u8], actual: &[u8]) -> Option<Mismatch> {
    if expected == actual {
        return None;
    }
    let el: Vec<&[u8]> = expected.split(|b| *b == b'\n').collect();
    let al: Vec<&[u8]> = actual.split(|b| *b == b'\n').collect();
    let first_diff = el.iter().zip(al.iter()).take_while(|(e, a)| e == a).count();
    let ctx = |xs: &[&[u8]]| -> Vec<String> {
        xs.iter()
            .skip(first_diff.saturating_sub(2))
            .take(5)
            .map(|l| String::from_utf8_lossy(l).into_owned())
            .collect()
    };
    Some(Mismatch {
        line: first_diff + 1,
        expected_context: ctx(&el),
        actual_context: ctx(&al),
        expected_length: expected.len(),
        actual_length: actual.len(),
    })
}

pub fn render_mismatch(expected_file: &Path, actual_file: &Path, m: &Mismatch) -> String {
    let mut s = format!(
        "golden mismatch at line {}\n  expected ({} bytes): {}\n  actual   ({} bytes): {}\n  --- expected, around the difference:\n",
        m.line,
        m.expected_length,
        expected_file.display(),
        m.actual_length,
        actual_file.display()
    );
    for l in &m.expected_context {
        s.push_str(&format!("    {l}\n"));
    }
    s.push_str("  --- actual, around the difference:\n");
    for l in &m.actual_context {
        s.push_str(&format!("    {l}\n"));
    }
    s.push_str("  the prototype is the writer: RUE_UPDATE_GOLDENS=1 cabal run rue-proto-goldens, in proto/, if the change is intended\n");
    s
}
