//! The front end against the terms (docs/ROADMAP.md Phase 2 acceptance):
//! for every tenant case and every negative, the plan IR the resolver
//! derives from the `.rue` text is structurally equal to the term's. The
//! term names the host, the plan and the requester the text is resolved
//! for. A disagreement is a defect in the front end or an unfaithful
//! term, fixed as such and named in the commit; this test is what makes
//! the texts the source when the terms retire.

use std::path::{Path, PathBuf};

use rue_core::json::canonical;
use rue_surface::resolve::{resolve, Options};
use rue_tenants::cases;
use rue_tenants::golden::repo_root;

/// The text a case is derived from: the nearest `plan.rue` above its
/// expected directory (`tenants/<t>/expected/<host>` and
/// `tenants/_negative/<name>/expected` both).
fn text_of(root: &Path, dir: &str) -> PathBuf {
    let mut p = root.join(dir);
    while p.pop() {
        let candidate = p.join("plan.rue");
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("{dir}: no plan.rue above it");
}

fn canonical_text(v: &impl serde::Serialize) -> String {
    String::from_utf8(canonical::encode(&serde_json::to_value(v).unwrap()).unwrap()).unwrap()
}

#[test]
fn every_text_resolves_to_its_term() {
    let root = repo_root().unwrap();
    let mut failures = Vec::new();
    for c in cases() {
        let path = text_of(&root, &c.dir);
        let opts = Options {
            host: Some(c.ir.plan.owner.clone()),
            plan: Some(c.ir.plan.id.clone()),
            requester: Some(c.ir.requester.clone()),
        };
        match resolve(&path, &opts) {
            Err(diags) => failures.push(format!(
                "{}: the front end refused:\n{}",
                c.dir,
                diags
                    .iter()
                    .map(|d| format!("  {}", d.render()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
            Ok(ir) if ir == c.ir => {}
            Ok(ir) => {
                let want = canonical_text(&c.ir);
                let got = canonical_text(&ir);
                let mut report = String::new();
                let (w, g): (Vec<&str>, Vec<&str>) =
                    (want.lines().collect(), got.lines().collect());
                let mut shown = 0;
                let mut i = 0;
                let mut j = 0;
                while (i < w.len() || j < g.len()) && shown < 12 {
                    if i < w.len() && j < g.len() && w[i] == g[j] {
                        i += 1;
                        j += 1;
                        continue;
                    }
                    // Show the divergence and resynchronize on the next shared line.
                    let ctx = if i > 0 { w[i - 1] } else { "" };
                    report.push_str(&format!("  at term line {}: {ctx}\n", i));
                    let k = (0..8).find(|d| i + d < w.len() && j < g.len() && w[i + d] == g[j]);
                    let l = (0..8).find(|d| j + d < g.len() && i < w.len() && g[j + d] == w[i]);
                    match (k, l) {
                        (Some(k), _) if k > 0 => {
                            for line in &w[i..i + k] {
                                report.push_str(&format!("    term: {}\n", line.trim()));
                            }
                            i += k;
                        }
                        (_, Some(l)) if l > 0 => {
                            for line in &g[j..j + l] {
                                report.push_str(&format!("    text: {}\n", line.trim()));
                            }
                            j += l;
                        }
                        _ => {
                            if i < w.len() {
                                report.push_str(&format!("    term: {}\n", w[i].trim()));
                                i += 1;
                            }
                            if j < g.len() {
                                report.push_str(&format!("    text: {}\n", g[j].trim()));
                                j += 1;
                            }
                        }
                    }
                    shown += 1;
                }
                failures.push(format!(
                    "{}: the front end and the term differ:\n{report}",
                    c.dir
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n\n{}",
        failures.len(),
        cases().len(),
        failures.join("\n\n")
    );
}
