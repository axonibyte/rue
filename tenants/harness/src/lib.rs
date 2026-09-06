//! The acceptance tenants and everything golden-tested about them, for the
//! Rust crates. The Phase 0 prototype is the writer of every golden; this
//! crate reads the same files and holds `rue-core` to them byte for byte.
//!
//! The case table below is the authority on which goldens exist: the tests
//! compare exactly these, any expected file no case claims is an orphan, and
//! any case without its `plan.json` is a missing input.

pub mod golden;

use std::fs;
use std::path::Path;

use rue_core::check::{check, deferred_steps};
use rue_core::diagnostics::Code;
use rue_core::explain::explain;
use rue_core::ir::{parse, PlanIr};
use rue_core::json::canonical;
use rue_core::prose::prose;
use rue_core::states::render_table;
use rue_core::verdict::to_json;

pub use golden::Artifact;

/// A tenant case: the tenant directory and the host directory under
/// `expected/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantCase {
    pub tenant: &'static str,
    pub host: &'static str,
}

/// A negative case: the code it must refuse with and its slug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NegativeCase {
    pub code: Code,
    pub slug: &'static str,
}

pub const TENANT_CASES: &[TenantCase] = &[
    TenantCase {
        tenant: "t1",
        host: "db-01",
    },
    TenantCase {
        tenant: "t2",
        host: "node-b-auto",
    },
    TenantCase {
        tenant: "t2",
        host: "node-b-manual",
    },
    TenantCase {
        tenant: "t3",
        host: "fw-01",
    },
    TenantCase {
        tenant: "t3",
        host: "fw-win-01",
    },
    TenantCase {
        tenant: "t4",
        host: "site-ctl",
    },
    TenantCase {
        tenant: "t4",
        host: "site-ctl-defer",
    },
];

/// The negative cases, in the prototype's order: docs/ROADMAP.md Phase 0
/// task 8 first, then one or more for every other code the checker emits.
pub const NEGATIVES: &[NegativeCase] = &[
    NegativeCase {
        code: Code::E0401,
        slug: "backstop-armed-after-reach",
    },
    NegativeCase {
        code: Code::E0401,
        slug: "reach-with-controller-undo",
    },
    NegativeCase {
        code: Code::E0404,
        slug: "auto-with-force",
    },
    NegativeCase {
        code: Code::E0410,
        slug: "reach-with-defer",
    },
    NegativeCase {
        code: Code::E0501,
        slug: "wane-and-commit",
    },
    NegativeCase {
        code: Code::E0501,
        slug: "neither-wane-nor-commit",
    },
    NegativeCase {
        code: Code::E0502,
        slug: "commit-not-last",
    },
    NegativeCase {
        code: Code::E0505,
        slug: "path-without-commit",
    },
    NegativeCase {
        code: Code::E0506,
        slug: "unbounded-wait",
    },
    NegativeCase {
        code: Code::E0507,
        slug: "auto-with-human-ack",
    },
    NegativeCase {
        code: Code::E0508,
        slug: "gate-counts-requester",
    },
    NegativeCase {
        code: Code::E0509,
        slug: "zero-human-gate",
    },
    NegativeCase {
        code: Code::E0201,
        slug: "no-undo-not-knell",
    },
    NegativeCase {
        code: Code::E0202,
        slug: "target-undo-not-closed",
    },
    NegativeCase {
        code: Code::E0203,
        slug: "none-locus-with-undo",
    },
    NegativeCase {
        code: Code::E0205,
        slug: "held-without-suspend",
    },
    NegativeCase {
        code: Code::E0207,
        slug: "compensate-without-undo-pre",
    },
    NegativeCase {
        code: Code::E0208,
        slug: "undo-not-idempotent",
    },
    NegativeCase {
        code: Code::E0407,
        slug: "target-undo-on-api-host",
    },
    NegativeCase {
        code: Code::E0301,
        slug: "umbra-conflict",
    },
    NegativeCase {
        code: Code::E0302,
        slug: "may-conflict-strict",
    },
    NegativeCase {
        code: Code::E0302,
        slug: "may-conflict-bound-host",
    },
    NegativeCase {
        code: Code::E0303,
        slug: "par-not-disjoint",
    },
    NegativeCase {
        code: Code::E0304,
        slug: "reach-inside-par",
    },
    NegativeCase {
        code: Code::E0305,
        slug: "anchor-twice",
    },
    NegativeCase {
        code: Code::E0403,
        slug: "scheduler-absent",
    },
    NegativeCase {
        code: Code::E0405,
        slug: "heartbeat-too-slow",
    },
    NegativeCase {
        code: Code::E0503,
        slug: "backstop-after-not-wane",
    },
    NegativeCase {
        code: Code::E0504,
        slug: "permanent-backstop-on-timer",
    },
    NegativeCase {
        code: Code::E0509,
        slug: "zero-human-step-gate",
    },
];

/// The codes the checker emits. Every other code in the table is a surface,
/// engine or analysis rule the crates do not model yet; the README lists
/// them under "not proven". The tests hold this equal to the codes the
/// negative cases refuse with, in both directions.
pub const EMITTED_CODES: &[Code] = &[
    Code::E0201,
    Code::E0202,
    Code::E0203,
    Code::E0205,
    Code::E0207,
    Code::E0208,
    Code::E0301,
    Code::E0302,
    Code::E0303,
    Code::E0304,
    Code::E0305,
    Code::E0401,
    Code::E0403,
    Code::E0404,
    Code::E0405,
    Code::E0407,
    Code::E0410,
    Code::E0501,
    Code::E0502,
    Code::E0503,
    Code::E0504,
    Code::E0505,
    Code::E0506,
    Code::E0507,
    Code::E0508,
    Code::E0509,
];

impl TenantCase {
    /// The case's `expected/` directory, relative to the repository root.
    pub fn dir(&self) -> String {
        format!("tenants/{}/expected/{}", self.tenant, self.host)
    }
    pub fn input(&self) -> String {
        format!("{}/plan.json", self.dir())
    }
}

impl NegativeCase {
    /// The case's directory name, `<code>-<slug>`.
    pub fn name(&self) -> String {
        format!("{}-{}", self.code, self.slug)
    }
    pub fn dir(&self) -> String {
        format!("tenants/_negative/{}/expected", self.name())
    }
    pub fn input(&self) -> String {
        format!("{}/plan.json", self.dir())
    }
}

/// The path of the generated transition table.
pub const STATE_TABLE: &str = "docs/state-transitions.tsv";

/// Every input the cases read, relative to the repository root.
pub fn inputs() -> Vec<String> {
    TENANT_CASES
        .iter()
        .map(TenantCase::input)
        .chain(NEGATIVES.iter().map(NegativeCase::input))
        .collect()
}

/// Read and parse a case's plan IR.
pub fn load(root: &Path, input: &str) -> Result<PlanIr, String> {
    let bytes = fs::read(root.join(input)).map_err(|e| format!("{input}: {e}"))?;
    parse(&bytes).map_err(|e| format!("{input}: {e}"))
}

fn verdict_artifacts(root: &Path, dir: &str, input: &str, with_explain: bool) -> Vec<Artifact> {
    match load(root, input) {
        Err(e) => {
            let mut v = vec![
                Artifact {
                    path: format!("{dir}/verdict.json"),
                    bytes: Err(e.clone()),
                },
                Artifact {
                    path: format!("{dir}/verdict.txt"),
                    bytes: Err(e.clone()),
                },
            ];
            if with_explain {
                v.push(Artifact {
                    path: format!("{dir}/explain.txt"),
                    bytes: Err(e),
                });
            }
            v
        }
        Ok(ir) => {
            let v = check(&ir.site, &ir.requester, &ir.plan);
            let mut out = vec![
                Artifact {
                    path: format!("{dir}/verdict.json"),
                    bytes: canonical::encode(&to_json(&v)).map_err(|e| e.to_string()),
                },
                Artifact {
                    path: format!("{dir}/verdict.txt"),
                    bytes: Ok(prose(&v).into_bytes()),
                },
            ];
            if with_explain {
                out.push(Artifact {
                    path: format!("{dir}/explain.txt"),
                    bytes: Ok(explain(&ir.plan, &deferred_steps(&ir.site, &ir.plan)).into_bytes()),
                });
            }
            out
        }
    }
}

/// Every golden artifact the Rust crates reproduce, with its path relative
/// to the repository root: the tenant cases' verdicts and explain listings,
/// the negatives' verdicts, and the transition table.
pub fn artifacts(root: &Path) -> Vec<Artifact> {
    let mut out = Vec::new();
    for c in TENANT_CASES {
        out.extend(verdict_artifacts(root, &c.dir(), &c.input(), true));
    }
    for n in NEGATIVES {
        out.extend(verdict_artifacts(root, &n.dir(), &n.input(), false));
    }
    out.push(Artifact {
        path: STATE_TABLE.to_string(),
        bytes: Ok(render_table().into_bytes()),
    });
    out
}
