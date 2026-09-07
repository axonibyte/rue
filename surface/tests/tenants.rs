//! The tenant corpus (docs/ROADMAP.md Phase 2 acceptance): every `.rue`
//! text under `tenants/` parses with no diagnostics, `rue fmt` is the
//! identity on it, and `fmt` is idempotent.

use std::fs;
use std::path::PathBuf;

use rue_surface::{format, parse};

fn corpus() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for t in ["t1", "t2", "t3", "t4"] {
        files.push(root.join("tenants").join(t).join("plan.rue"));
    }
    for e in fs::read_dir(root.join("tenants/_negative")).unwrap() {
        let p = e.unwrap().path().join("plan.rue");
        if p.is_file() {
            files.push(p);
        }
    }
    files.sort();
    assert_eq!(
        files.len(),
        40,
        "the corpus is the four tenants and thirty-six negatives"
    );
    files
}

#[test]
fn every_text_parses_clean() {
    for f in corpus() {
        let src = fs::read_to_string(&f).unwrap();
        let p = parse(&src, f.to_str().unwrap());
        let rendered: Vec<String> = p.diagnostics.iter().map(|d| d.render()).collect();
        assert!(
            rendered.is_empty(),
            "{}:\n{}",
            f.display(),
            rendered.join("\n")
        );
    }
}

#[test]
fn fmt_is_the_identity_on_every_text_and_idempotent() {
    for f in corpus() {
        let src = fs::read_to_string(&f).unwrap();
        let once = format(&src, f.to_str().unwrap()).unwrap_or_else(|d| {
            panic!(
                "{}: {}",
                f.display(),
                d.iter().map(|d| d.render()).collect::<Vec<_>>().join("\n")
            )
        });
        if once != src {
            let mut diff = String::new();
            for (n, (a, b)) in src.lines().zip(once.lines()).enumerate() {
                if a != b {
                    diff.push_str(&format!(
                        "line {}:\n  source:    {a:?}\n  formatted: {b:?}\n",
                        n + 1
                    ));
                }
            }
            if src.lines().count() != once.lines().count() {
                diff.push_str(&format!(
                    "line counts differ: {} vs {}\n",
                    src.lines().count(),
                    once.lines().count()
                ));
            }
            panic!("{}: fmt is not the identity:\n{diff}", f.display());
        }
        let twice = format(&once, f.to_str().unwrap()).unwrap();
        assert_eq!(twice, once, "{}: fmt is not idempotent", f.display());
    }
}
