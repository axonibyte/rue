//! Parser goldens: every construct of the surface and every recovery, as a
//! `.rue` snippet under `tests/corpus/` with its tree dump (`.tree`) and
//! its diagnostics (`.diag`) beside it. The goldens are read-only in the
//! suite; `RUE_UPDATE_GOLDENS=1` rewrites them, as it does the tenants'.
//! A clean snippet must also survive `fmt` unchanged (the corpus is written
//! in the canonical layout) and idempotently.

use std::fs;
use std::path::{Path, PathBuf};

use rue_surface::{dump, format, parse};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn hold(path: &Path, actual: &str) -> Result<(), String> {
    if std::env::var("RUE_UPDATE_GOLDENS").as_deref() == Ok("1") {
        fs::write(path, actual).unwrap();
        return Ok(());
    }
    match fs::read_to_string(path) {
        Ok(expected) if expected == actual => Ok(()),
        Ok(expected) => {
            let first = expected
                .lines()
                .zip(actual.lines())
                .position(|(a, b)| a != b)
                .map(|i| i + 1)
                .unwrap_or(expected.lines().count().min(actual.lines().count()) + 1);
            Err(format!(
                "{} differs from the golden at line {first}; actual:\n{actual}",
                path.display()
            ))
        }
        Err(_) => Err(format!(
            "{} is missing; run with RUE_UPDATE_GOLDENS=1 to write it",
            path.display()
        )),
    }
}

#[test]
fn every_snippet_matches_its_tree_and_diagnostics_goldens() {
    let mut names: Vec<PathBuf> = fs::read_dir(corpus_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rue"))
        .collect();
    names.sort();
    assert!(names.len() >= 12, "the corpus has at least twelve snippets");
    let mut failures = Vec::new();
    for rue in &names {
        let src = fs::read_to_string(rue).unwrap();
        let name = rue.file_name().unwrap().to_str().unwrap();
        let p = parse(&src, name);
        let tree = dump(&p.root);
        let diag: String = p
            .diagnostics
            .iter()
            .map(|d| format!("{}\n", d.render()))
            .collect();
        if let Err(e) = hold(&rue.with_extension("tree"), &tree) {
            failures.push(e);
        }
        if let Err(e) = hold(&rue.with_extension("diag"), &diag) {
            failures.push(e);
        }
        let is_error_case = name.starts_with("err-");
        assert_eq!(
            p.diagnostics.is_empty(),
            !is_error_case,
            "{name}: an err- snippet has diagnostics and every other has none"
        );
        // The tree is lossless: its text is the source.
        assert_eq!(p.root.text().to_string(), src, "{name}: the tree lost text");
        if !is_error_case {
            let once = format(&src, name).unwrap();
            assert_eq!(once, src, "{name}: fmt is not the identity on the corpus");
            assert_eq!(
                format(&once, name).unwrap(),
                once,
                "{name}: fmt is not idempotent"
            );
        } else {
            assert!(
                format(&src, name).is_err(),
                "{name}: fmt must refuse a file with errors"
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
