//! The acceptance tenants and everything golden-tested about them.
//!
//! The terms under [`tenants`] are the record of what Phase 0 proved and the
//! source of every golden: `plan.json` is a case's term as the plan IR,
//! `verdict.json`, `verdict.txt` and `explain.txt` are what `rue-core` says
//! about it, and `docs/state-transitions.tsv` is the state machine's table.
//! The case table below ([`TENANT_CASES`], [`NEGATIVES`]) is the authority on
//! which goldens exist; the tests hold the terms and the table 1:1, compare
//! every artifact byte for byte, and refuse an expected file nothing
//! declares. `rue-goldens` is the only writer, and only when told to.

pub mod golden;
pub mod tenants;

use std::fs;
use std::path::Path;

use rue_core::check::{check, deferred_steps};
use rue_core::diagnostics::Code;
use rue_core::explain::explain;
use rue_core::ir::{parse, PlanIr, IR_VERSION};
use rue_core::json::canonical;
use rue_core::model::{Plan, Site};
use rue_core::prose::prose;
use rue_core::states::render_table;
use rue_core::verdict::{to_json, Verdict};

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

/// The negative cases: docs/ROADMAP.md Phase 0 task 8 first, then one or
/// more for every other code the checker emits.
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

/// A case as a term: its directory, its IR, and whether it has an explain
/// golden (tenant cases do; negatives do not).
#[derive(Debug, Clone)]
pub struct CaseTerm {
    pub dir: String,
    pub ir: PlanIr,
    pub with_explain: bool,
}

fn ir_of(site: &Site, requester: &str, plan: &Plan) -> PlanIr {
    PlanIr {
        ir_version: IR_VERSION,
        requester: requester.to_string(),
        site: site.clone(),
        plan: plan.clone(),
    }
}

/// Every case, tenants first in the table's order, then the negatives in
/// theirs. A term without a table row, or a row without a term, is caught by
/// the tests.
pub fn cases() -> Vec<CaseTerm> {
    let mut out = Vec::new();
    for t in tenants::tenants() {
        for c in &t.cases {
            out.push(CaseTerm {
                dir: format!("tenants/{}/expected/{}", t.name, c.host),
                ir: ir_of(&t.site, &t.requester, &c.plan),
                with_explain: true,
            });
        }
    }
    for n in tenants::negatives() {
        out.push(CaseTerm {
            dir: format!("tenants/_negative/{}-{}/expected", n.code, n.slug),
            ir: ir_of(&n.site, &n.requester, &n.plan),
            with_explain: false,
        });
    }
    out
}

/// A case's verdict.
pub fn verdict_of(ir: &PlanIr) -> Verdict {
    check(&ir.site, &ir.requester, &ir.plan)
}

/// Read and parse a case's plan IR from disk.
pub fn load(root: &Path, input: &str) -> Result<PlanIr, String> {
    let bytes = fs::read(root.join(input)).map_err(|e| format!("{input}: {e}"))?;
    parse(&bytes).map_err(|e| format!("{input}: {e}"))
}

/// Every golden artifact, with its path relative to the repository root: for
/// each case its IR, verdict, prose and (tenant cases) explain listing, and
/// the transition table.
pub fn artifacts() -> Vec<Artifact> {
    let mut out = Vec::new();
    for c in cases() {
        let v = verdict_of(&c.ir);
        out.push(Artifact {
            path: format!("{}/plan.json", c.dir),
            bytes: serde_json::to_value(&c.ir)
                .map_err(|e| e.to_string())
                .and_then(|j| canonical::encode(&j).map_err(|e| e.to_string())),
        });
        out.push(Artifact {
            path: format!("{}/verdict.json", c.dir),
            bytes: canonical::encode(&to_json(&v)).map_err(|e| e.to_string()),
        });
        out.push(Artifact {
            path: format!("{}/verdict.txt", c.dir),
            bytes: Ok(prose(&v).into_bytes()),
        });
        if c.with_explain {
            out.push(Artifact {
                path: format!("{}/explain.txt", c.dir),
                bytes: Ok(explain(&c.ir.plan, &deferred_steps(&c.ir.site, &c.ir.plan)).into_bytes()),
            });
        }
    }
    out.push(Artifact {
        path: STATE_TABLE.to_string(),
        bytes: Ok(render_table().into_bytes()),
    });
    out
}
