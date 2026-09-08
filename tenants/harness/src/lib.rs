//! The acceptance tenants and everything golden-tested about them.
//!
//! The `.rue` texts under `tenants/` are the source of every golden (from
//! Phase 2; the Rust terms that carried Phase 0's record retired once the
//! front end reproduced each of them): `plan.json` is a case's text
//! resolved for its host as the plan IR, `verdict.json`, `verdict.txt` and
//! `explain.txt` are what `rue-core` says about it, an artifact file is what
//! `rue-render` installs, `diagnostics.txt` is what the front end says of a
//! text it refuses, and `docs/state-transitions.tsv` is the state machine's
//! table. The case tables below ([`TENANT_CASES`], [`NEGATIVES`],
//! [`SURFACE_NEGATIVES`]) are the authority on which goldens exist and name
//! the host, plan and requester each text is resolved for; the tests
//! compare every artifact byte for byte and refuse an expected file nothing
//! declares. `rue-goldens` is the only writer, and only when told to.

pub mod golden;

use std::fs;
use std::path::Path;

use rue_core::check::{check, deferred_steps};
use rue_core::diagnostics::Code;
use rue_core::explain::explain;
use rue_core::ir::{parse, PlanIr};
use rue_core::json::canonical;
use rue_core::model::Plan;
use rue_core::prose::prose;
use rue_core::states::render_table;
use rue_core::verdict::{to_json, Verdict};
use rue_render::{Bindings, Instance};

pub use golden::Artifact;

/// A tenant case: the tenant directory and the host directory under
/// `expected/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantCase {
    pub tenant: &'static str,
    /// The case directory under `expected/`.
    pub host: &'static str,
    /// The inventory host the text is resolved for.
    pub owner: &'static str,
    /// The plan name in the text.
    pub plan: &'static str,
    pub requester: &'static str,
}

/// A negative case: the code it must refuse with and its slug.
/// The requester every negative is checked as (section 5.11: the requester
/// is an input to `check`; the negatives derived from a tenant keep this
/// name so E0508 counts it as they always did).
pub const NEGATIVE_REQUESTER: &str = "requester";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NegativeCase {
    pub code: Code,
    pub slug: &'static str,
    /// The inventory host the text is resolved for.
    pub owner: &'static str,
    /// The plan name in the text.
    pub plan: &'static str,
}

/// A negative the front end refuses before a plan exists: its golden is
/// the rendered diagnostics, not a verdict. The text is resolved for
/// `host` on the lab site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceNegative {
    pub code: Code,
    pub slug: &'static str,
    pub host: &'static str,
}

impl SurfaceNegative {
    pub fn name(&self) -> String {
        format!("{}-{}", self.code, self.slug)
    }
    pub fn dir(&self) -> String {
        format!("tenants/_negative/{}/expected", self.name())
    }
    pub fn text(&self) -> String {
        format!("tenants/_negative/{}/plan.rue", self.name())
    }
}

macro_rules! surface_negatives {
    ($($code:ident => $slug:literal),* $(,)?) => {
        pub const SURFACE_NEGATIVES: &[SurfaceNegative] = &[
            $(SurfaceNegative { code: Code::$code, slug: $slug, host: "db-01" },)*
        ];
    };
}

surface_negatives! {
    E0101 => "parse-error",
    E0102 => "unknown-op",
    E0103 => "duplicate-clause",
    E0104 => "import-cycle",
    E0105 => "version-missing",
    E0106 => "unbounded-repeat",
    E0107 => "kind-mismatch",
    E0108 => "compare-unknown",
    E0110 => "output-before-step",
    E0111 => "clause-on-probed-fact",
    E0112 => "no-clause-for-host",
    E0113 => "repeat-not-set-valued",
    E0114 => "when-arms-differ",
    E0204 => "knell-without-cost",
    E0601 => "unknown-binding",
    E0602 => "binding-contract",
    E0603 => "no-journal",
    E0604 => "no-operators",
    E0605 => "hook-without-registrar",
}

/// The codes the front end raises, each with a surface negative.
pub const SURFACE_CODES: &[Code] = &[
    Code::E0101,
    Code::E0102,
    Code::E0103,
    Code::E0104,
    Code::E0105,
    Code::E0106,
    Code::E0107,
    Code::E0108,
    Code::E0110,
    Code::E0111,
    Code::E0112,
    Code::E0113,
    Code::E0114,
    Code::E0204,
    Code::E0601,
    Code::E0602,
    Code::E0603,
    Code::E0604,
    Code::E0605,
];

/// The codes the renderer raises, unit-tested in `render/tests`.
pub const RENDER_CODES: &[Code] = &[Code::E0109];

/// The codes nothing in the workspace raises, each with the reason.
pub const UNMODELED_CODES: &[(Code, &str)] = &[
    (
        Code::E0402,
        "renewal ordering against a rearm is engine time (Phase 3)",
    ),
    (
        Code::E0406,
        "installation precedes the first covered step by construction; unreachable in this model",
    ),
    (
        Code::E0408,
        "snapshot sizes are observed at apply (Phase 3)",
    ),
    (
        Code::E0409,
        "a multi-host plan's owner is fixed by --host; inference is Phase 3's",
    ),
    (Code::E0411, "the site does not declare sinks"),
];

/// The diagnostics a surface negative's text raises, rendered with paths
/// relative to the repository root so the golden is location-free.
pub fn surface_diagnostics(root: &Path, n: &SurfaceNegative) -> Result<String, String> {
    let opts = rue_surface::resolve::Options {
        host: Some(n.host.to_string()),
        plan: None,
        requester: None,
    };
    match rue_surface::resolve::resolve(&root.join(n.text()), &opts) {
        Ok(_) => Err(format!(
            "{}: the front end accepted a text that must refuse",
            n.name()
        )),
        Err(diags) => {
            // The root as the resolver names it, then the platform's
            // separator; what remains is repo-relative. Windows writes
            // backslashes between the components and the golden, written
            // once for every platform, holds none (no message contains a
            // backslash), so they become slashes there.
            let prefix = format!(
                "{}{}",
                rue_surface::resolve::display_path(root).display(),
                std::path::MAIN_SEPARATOR
            );
            Ok(diags
                .iter()
                .map(|d| {
                    let line = d.render().replace(&prefix, "");
                    let line = if cfg!(windows) {
                        line.replace('\\', "/")
                    } else {
                        line
                    };
                    format!("{line}\n")
                })
                .collect())
        }
    }
}

pub const TENANT_CASES: &[TenantCase] = &[
    TenantCase {
        tenant: "t1",
        host: "db-01",
        owner: "db-01",
        plan: "breakglass",
        requester: "ops_requester",
    },
    TenantCase {
        tenant: "t2",
        host: "node-b-auto",
        owner: "node-b",
        plan: "promote_auto",
        requester: "operator",
    },
    TenantCase {
        tenant: "t2",
        host: "node-b-manual",
        owner: "node-b",
        plan: "promote",
        requester: "operator",
    },
    TenantCase {
        tenant: "t3",
        host: "fw-01",
        owner: "fw-01",
        plan: "open_mgmt_port",
        requester: "netops_requester",
    },
    TenantCase {
        tenant: "t3",
        host: "fw-win-01",
        owner: "fw-win-01",
        plan: "open_mgmt_port",
        requester: "netops_requester",
    },
    TenantCase {
        tenant: "t3",
        host: "fw-02",
        owner: "fw-02",
        plan: "open_mgmt_port",
        requester: "netops_requester",
    },
    TenantCase {
        tenant: "t3",
        host: "fw-win-02",
        owner: "fw-win-02",
        plan: "open_mgmt_port",
        requester: "netops_requester",
    },
    TenantCase {
        tenant: "t3",
        host: "fw-mac-01",
        owner: "fw-mac-01",
        plan: "open_mgmt_port",
        requester: "netops_requester",
    },
    TenantCase {
        tenant: "t4",
        host: "site-ctl",
        owner: "site-ctl",
        plan: "shed_load",
        requester: "reactive_host",
    },
    TenantCase {
        tenant: "t4",
        host: "site-ctl-defer",
        owner: "site-ctl",
        plan: "shed_load_deferring",
        requester: "reactive_host",
    },
];

/// The negative cases: docs/ROADMAP.md Phase 0 task 8 first, then one or
/// more for every other code the checker emits.
pub const NEGATIVES: &[NegativeCase] = &[
    NegativeCase {
        code: Code::E0401,
        slug: "backstop-armed-after-reach",
        owner: "fw-01",
        plan: "open_mgmt_port",
    },
    NegativeCase {
        code: Code::E0401,
        slug: "reach-with-controller-undo",
        owner: "fw-01",
        plan: "open_mgmt_port",
    },
    NegativeCase {
        code: Code::E0404,
        slug: "auto-with-force",
        owner: "db-01",
        plan: "breakglass",
    },
    NegativeCase {
        code: Code::E0410,
        slug: "reach-with-defer",
        owner: "fw-01",
        plan: "open_mgmt_port",
    },
    NegativeCase {
        code: Code::E0501,
        slug: "wane-and-commit",
        owner: "fw-01",
        plan: "open_mgmt_port",
    },
    NegativeCase {
        code: Code::E0501,
        slug: "neither-wane-nor-commit",
        owner: "db-01",
        plan: "breakglass",
    },
    NegativeCase {
        code: Code::E0502,
        slug: "commit-not-last",
        owner: "fw-01",
        plan: "open_mgmt_port",
    },
    NegativeCase {
        code: Code::E0505,
        slug: "path-without-commit",
        owner: "fw-01",
        plan: "open_mgmt_port",
    },
    NegativeCase {
        code: Code::E0506,
        slug: "unbounded-wait",
        owner: "fw-01",
        plan: "open_mgmt_port",
    },
    NegativeCase {
        code: Code::E0507,
        slug: "auto-with-human-ack",
        owner: "node-b",
        plan: "promote",
    },
    NegativeCase {
        code: Code::E0508,
        slug: "gate-counts-requester",
        owner: "db-01",
        plan: "breakglass",
    },
    NegativeCase {
        code: Code::E0509,
        slug: "zero-human-gate",
        owner: "db-01",
        plan: "breakglass",
    },
    NegativeCase {
        code: Code::E0201,
        slug: "no-undo-not-knell",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0202,
        slug: "target-undo-not-closed",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0203,
        slug: "none-locus-with-undo",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0205,
        slug: "held-without-suspend",
        owner: "db-01",
        plan: "tunnel",
    },
    NegativeCase {
        code: Code::E0207,
        slug: "compensate-without-undo-pre",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0208,
        slug: "undo-not-idempotent",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0407,
        slug: "target-undo-on-api-host",
        owner: "db-01",
        plan: "bmc",
    },
    NegativeCase {
        code: Code::E0301,
        slug: "umbra-conflict",
        owner: "db-01",
        plan: "twice",
    },
    NegativeCase {
        code: Code::E0302,
        slug: "may-conflict-strict",
        owner: "db-01",
        plan: "any",
    },
    NegativeCase {
        code: Code::E0302,
        slug: "may-conflict-bound-host",
        owner: "db-01",
        plan: "any",
    },
    NegativeCase {
        code: Code::E0303,
        slug: "par-not-disjoint",
        owner: "db-01",
        plan: "par",
    },
    NegativeCase {
        code: Code::E0304,
        slug: "reach-inside-par",
        owner: "db-01",
        plan: "par",
    },
    NegativeCase {
        code: Code::E0305,
        slug: "anchor-twice",
        owner: "db-01",
        plan: "regions",
    },
    NegativeCase {
        code: Code::E0403,
        slug: "scheduler-absent",
        owner: "island",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0405,
        slug: "heartbeat-too-slow",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0503,
        slug: "backstop-after-not-wane",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0504,
        slug: "permanent-backstop-on-timer",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0509,
        slug: "zero-human-step-gate",
        owner: "db-01",
        plan: "fence",
    },
    NegativeCase {
        code: Code::E0206,
        slug: "reestablish-reruns-do",
        owner: "db-01",
        plan: "tunnel",
    },
    NegativeCase {
        code: Code::E0209,
        slug: "secret-in-run-string",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0210,
        slug: "secret-in-target-undo",
        owner: "db-01",
        plan: "posture",
    },
    NegativeCase {
        code: Code::E0211,
        slug: "executor-without-stdin-preamble",
        owner: "db-01",
        plan: "bmc_login",
    },
    NegativeCase {
        code: Code::E0403,
        slug: "artifact-language-unsupported",
        owner: "fw-win-01",
        plan: "open_mgmt_port",
    },
    NegativeCase {
        code: Code::E0606,
        slug: "secret-without-deliver-to",
        owner: "db-01",
        plan: "token",
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
    Code::E0206,
    Code::E0207,
    Code::E0208,
    Code::E0209,
    Code::E0210,
    Code::E0211,
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
    Code::E0606,
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

/// A case: its directory, the plan IR resolved from its text, and whether
/// it has an explain golden (tenant cases do; negatives do not).
#[derive(Debug, Clone)]
pub struct Case {
    pub dir: String,
    pub ir: PlanIr,
    pub with_explain: bool,
}

/// The text a case is derived from: the nearest `plan.rue` above its
/// expected directory.
pub fn text_of(root: &Path, dir: &str) -> std::path::PathBuf {
    let mut p = root.join(dir);
    while p.pop() {
        let candidate = p.join("plan.rue");
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("{dir}: no plan.rue above it");
}

fn resolve_case(root: &Path, dir: &str, owner: &str, plan: &str, requester: &str) -> PlanIr {
    let opts = rue_surface::resolve::Options {
        host: Some(owner.to_string()),
        plan: Some(plan.to_string()),
        requester: Some(requester.to_string()),
    };
    rue_surface::resolve::resolve(&text_of(root, dir), &opts).unwrap_or_else(|diags| {
        panic!(
            "{dir}: the text does not resolve:\n{}",
            diags
                .iter()
                .map(|d| format!("  {}", d.render()))
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

/// Every case, tenants first in the table's order, then the negatives in
/// theirs, each resolved from its `.rue` text (the texts are the golden
/// source from Phase 2 on). A text that does not resolve is a panic here
/// and a failure of every suite that reads it.
pub fn cases() -> Vec<Case> {
    let root = golden::repo_root().unwrap_or_default();
    let mut out = Vec::new();
    for t in TENANT_CASES {
        let dir = t.dir();
        out.push(Case {
            ir: resolve_case(&root, &dir, t.owner, t.plan, t.requester),
            dir,
            with_explain: true,
        });
    }
    for n in NEGATIVES {
        let dir = n.dir();
        out.push(Case {
            ir: resolve_case(&root, &dir, n.owner, n.plan, NEGATIVE_REQUESTER),
            dir,
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
/// The instance every artifact golden is rendered for.
pub const GOLDEN_INSTANCE: &str = "golden";

/// A plan's step arguments as the bindings a render bakes: every
/// `name: value` argument of every step, as the request would bind them.
pub fn bindings_of(plan: &Plan) -> Bindings {
    let mut b = Bindings::default();
    for (_, it) in rue_core::algebra::numbered(&plan.body) {
        if let Some(s) = rue_core::algebra::step_of(it) {
            for a in &s.args {
                if let Some((k, v)) = a.split_once(": ") {
                    b.params.insert(k.to_string(), v.to_string());
                }
            }
        }
    }
    b
}

/// The backstop artifact of a case whose plan has a `:target` backstop, if
/// its plan checks clean: rendered for the plan's owner and the golden
/// instance. A clean plan whose artifact cannot be rendered is an error the
/// golden suite reports.
pub fn artifact_of(ir: &PlanIr) -> Option<Result<rue_render::Artifact, rue_render::RenderError>> {
    if verdict_of(ir).status != rue_core::verdict::Status::Ok {
        return None;
    }
    let instance = Instance {
        id: GOLDEN_INSTANCE.into(),
        rue_root: None,
    };
    match rue_render::render(
        &ir.site,
        &ir.plan,
        &ir.plan.owner,
        &instance,
        &bindings_of(&ir.plan),
    ) {
        Err(rue_render::RenderError::NoBackstop) | Err(rue_render::RenderError::NotTarget) => None,
        r => Some(r),
    }
}

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
        if let Some(a) = artifact_of(&c.ir) {
            out.push(Artifact {
                path: format!(
                    "{}/{}",
                    c.dir,
                    a.as_ref().map(|a| a.file_name).unwrap_or("artifact")
                ),
                bytes: a.map(|a| a.text.into_bytes()).map_err(|e| e.to_string()),
            });
        }
    }
    let root = golden::repo_root().unwrap_or_default();
    for n in SURFACE_NEGATIVES {
        out.push(Artifact {
            path: format!("{}/diagnostics.txt", n.dir()),
            bytes: surface_diagnostics(&root, n).map(String::into_bytes),
        });
    }
    out.push(Artifact {
        path: STATE_TABLE.to_string(),
        bytes: Ok(render_table().into_bytes()),
    });
    out
}
