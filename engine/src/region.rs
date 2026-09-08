//! Fenced regions and file digests, as the artifact templates define them
//! (docs/DESIGN.md, "The instance directory the artifact reads"): the
//! lines `# rue-region <anchor> begin` and `# rue-region <anchor> end`, a
//! region intact when exactly one of each is present, damaged otherwise.
//! `local()` applies these in process; `ssh()` runs the same rule as shell
//! on the target; the artifact does the same when it fires. One rule, three
//! places, held equal by the drift tests.

use sha2::{Digest, Sha256};

pub fn begin_marker(anchor: &str) -> String {
    format!("# rue-region {anchor} begin")
}

pub fn end_marker(anchor: &str) -> String {
    format!("# rue-region {anchor} end")
}

/// Whether the region's markers are intact: exactly one begin and one end.
pub fn intact(text: &str, anchor: &str) -> bool {
    let b = begin_marker(anchor);
    let e = end_marker(anchor);
    text.lines().filter(|l| *l == b).count() == 1 && text.lines().filter(|l| *l == e).count() == 1
}

/// Strip the region between its markers, markers included. `None` when the
/// markers are damaged (the caller decides the fallback).
pub fn strip(text: &str, anchor: &str) -> Option<String> {
    if !intact(text, anchor) {
        return None;
    }
    let b = begin_marker(anchor);
    let e = end_marker(anchor);
    let mut out = String::new();
    let mut skipping = false;
    for line in text.lines() {
        if line == b {
            skipping = true;
            continue;
        }
        if line == e {
            skipping = false;
            continue;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    Some(out)
}

/// Set the region: strip any intact one, then append the fenced block.
pub fn set(text: &str, anchor: &str, content: &str) -> String {
    let mut out = strip(text, anchor).unwrap_or_else(|| text.to_string());
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&begin_marker(anchor));
    out.push('\n');
    out.push_str(content);
    out.push('\n');
    out.push_str(&end_marker(anchor));
    out.push('\n');
    out
}

/// The hex SHA-256 of bytes, as `sha256 -q` prints it.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    crate::sign::hex(&h.finalize())
}

/// The path a `file:` shape names.
pub fn file_path(shape: &str) -> Option<&str> {
    shape.strip_prefix("file:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_strip_is_the_identity_and_damage_is_detected() {
        let base = "top\nkeep\n";
        let with = set(base, "blk", "inside\nmore");
        assert_eq!(
            with,
            "top\nkeep\n# rue-region blk begin\ninside\nmore\n# rue-region blk end\n"
        );
        assert!(intact(&with, "blk"));
        assert_eq!(strip(&with, "blk").unwrap(), base);
        // Setting again replaces rather than duplicates.
        let again = set(&with, "blk", "new");
        assert_eq!(again.matches("# rue-region blk begin").count(), 1);
        assert!(again.contains("new") && !again.contains("inside"));
        // A lost end marker is damage.
        let damaged = with.replace("# rue-region blk end\n", "");
        assert!(!intact(&damaged, "blk"));
        assert_eq!(strip(&damaged, "blk"), None);
        // Another anchor is untouched.
        assert!(!intact(&with, "other"));
        assert_eq!(
            set("", "a", "x"),
            "# rue-region a begin\nx\n# rue-region a end\n"
        );
    }

    #[test]
    fn sha256_matches_the_known_digest_of_empty_and_abc() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(file_path("file:/etc/x"), Some("/etc/x"));
        assert_eq!(file_path("proc:x"), None);
    }
}
