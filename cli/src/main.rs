//! `rue`, the operator CLI (docs/ROADMAP.md section 6.8), as far as Phase 1
//! takes it: `check`, `explain` and `artifact` over a plan IR document or,
//! from Phase 2, a `.rue` file resolved for one host (`--host`,
//! `--plan-name`, `--as`); `states`; and `fmt` over a `.rue` file. The surface verbs (`.rue` input, `--host`, `--as`) arrive with
//! Phase 2; the IR is already one host's plan and carries the requester.
//!
//! Exit codes are the roadmap's: 0 ok; 1 refused; 2 usage, an unreadable or
//! unparseable input, an IR version this build does not read. stdout is
//! data, written as bytes so no platform translates a newline; stderr is
//! diagnostics; every verb that judges a plan ends in the verdict line, and
//! it is the last line printed.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rue_core::check::{check, deferred_steps};
use rue_core::explain::explain;
use rue_core::ir::{parse, PlanIr};
use rue_core::json::canonical;
use rue_core::prose::prose;
use rue_core::states::render_table;
use rue_core::verdict::{to_json, Status, Verdict};
use rue_render::{render, Bindings, Instance, RenderError};

#[derive(Parser)]
#[command(
    name = "rue",
    version,
    about = "A language for provably reversible operations",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
    /// Check a plan and print its verdict.
    Check {
        /// A .rue file, or a plan IR document (docs/TESTING.md, "The plan IR").
        plan: PathBuf,
        /// Print the structured verdict in canonical JSON instead of the prose.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        select: Select,
    },
    /// List a plan's numbered steps with their undo lines, loci and policies.
    Explain {
        /// A .rue file, or a plan IR document.
        plan: PathBuf,
        #[command(flatten)]
        select: Select,
    },
    /// Print the runtime state machine's transition table.
    States,
    /// Format a .rue file (section 6.9): the canonical layout, comments
    /// kept; the identity on a formatted file.
    Fmt {
        /// The .rue file.
        file: PathBuf,
        /// Print nothing; exit 1 if the file is not already formatted.
        #[arg(long)]
        check: bool,
    },
    /// Print the backstop artifact a :target backstop installs on the host
    /// (section 7.7): the scheduler-run script that undoes the covered
    /// steps when its trigger is due.
    Artifact {
        /// A .rue file, or a plan IR document. `--host` names the record the
        /// artifact is rendered for (the plan's owner when absent) and, for a
        /// .rue file, the plan's host.
        plan: PathBuf,
        #[command(flatten)]
        select: Select,
        /// The instance id the artifact is rendered for.
        #[arg(long)]
        instance: String,
        /// The target's rue_root; the family's default when absent.
        #[arg(long)]
        rue_root: Option<String>,
        /// A plan parameter the artifact bakes in, `name=value`; repeatable.
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
    },
}

/// What selects one host's plan from a `.rue` file (section 6.8).
#[derive(clap::Args, Default)]
struct Select {
    /// The inventory host the plan runs on (a .rue input).
    #[arg(long)]
    host: Option<String>,
    /// The plan, when the file defines more than one (a .rue input).
    #[arg(long)]
    plan_name: Option<String>,
    /// The requester's identity; the first declared operator when absent.
    #[arg(long = "as")]
    requester: Option<String>,
}

/// A plan IR document, or a `.rue` file resolved for one host. A resolver
/// diagnostic is a refusal (exit 1) with the diagnostics on stderr.
fn load_input(
    path: &PathBuf,
    select: &Select,
    host_on_ir: bool,
) -> Result<Result<PlanIr, ExitCode>> {
    if path.extension().is_some_and(|e| e == "rue") {
        let opts = rue_surface::resolve::Options {
            host: select.host.clone(),
            plan: select.plan_name.clone(),
            requester: select.requester.clone(),
        };
        return Ok(match rue_surface::resolve::resolve(path, &opts) {
            Ok(ir) => Ok(ir),
            Err(diags) => {
                for d in &diags {
                    eprintln!("{}", d.render());
                }
                Err(ExitCode::from(1))
            }
        });
    }
    if (select.host.is_some() && !host_on_ir)
        || select.plan_name.is_some()
        || select.requester.is_some()
    {
        anyhow::bail!("--host, --plan-name and --as select from a .rue file; a plan IR is already one host's plan");
    }
    Ok(Ok(load(path)?))
}

fn load(path: &PathBuf) -> Result<PlanIr> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    parse(&bytes).with_context(|| format!("{}", path.display()))
}

fn verdict_of(ir: &PlanIr) -> Verdict {
    check(&ir.site, &ir.requester, &ir.plan)
}

fn status_code(v: &Verdict) -> ExitCode {
    match v.status {
        Status::Ok => ExitCode::SUCCESS,
        Status::Refused => ExitCode::from(1),
    }
}

fn run(cli: Cli, out: &mut dyn Write) -> Result<ExitCode> {
    match cli.verb {
        Verb::Check { plan, json, select } => {
            let ir = match load_input(&plan, &select, false)? {
                Ok(ir) => ir,
                Err(code) => return Ok(code),
            };
            let v = verdict_of(&ir);
            if json {
                out.write_all(&canonical::encode(&to_json(&v))?)?;
            } else {
                out.write_all(prose(&v).as_bytes())?;
            }
            Ok(status_code(&v))
        }
        Verb::Explain { plan, select } => {
            let ir = match load_input(&plan, &select, false)? {
                Ok(ir) => ir,
                Err(code) => return Ok(code),
            };
            let v = verdict_of(&ir);
            out.write_all(explain(&ir.plan, &deferred_steps(&ir.site, &ir.plan)).as_bytes())?;
            if v.status == Status::Refused {
                // The listing is still useful; the verdict says why it does
                // not stand, on stderr so stdout stays the listing.
                eprint!("{}", prose(&v));
            }
            Ok(status_code(&v))
        }
        Verb::States => {
            out.write_all(render_table().as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::Fmt { file, check } => {
            let src = fs::read_to_string(&file)
                .with_context(|| format!("cannot read {}", file.display()))?;
            match rue_surface::format(&src, &file.display().to_string()) {
                Ok(formatted) => {
                    if check {
                        if formatted == src {
                            Ok(ExitCode::SUCCESS)
                        } else {
                            eprintln!("rue: {} is not formatted", file.display());
                            Ok(ExitCode::from(1))
                        }
                    } else {
                        out.write_all(formatted.as_bytes())?;
                        Ok(ExitCode::SUCCESS)
                    }
                }
                Err(diagnostics) => {
                    for d in &diagnostics {
                        eprintln!("{}", d.render());
                    }
                    Ok(ExitCode::from(1))
                }
            }
        }
        Verb::Artifact {
            plan,
            select,
            instance,
            rue_root,
            set,
        } => {
            let ir = match load_input(&plan, &select, true)? {
                Ok(ir) => ir,
                Err(code) => return Ok(code),
            };
            let host = select.host.clone();
            let mut bindings = Bindings::default();
            for kv in &set {
                let (k, v) = kv
                    .split_once('=')
                    .with_context(|| format!("--set {kv}: expected NAME=VALUE"))?;
                bindings.params.insert(k.to_string(), v.to_string());
            }
            let host = host.unwrap_or_else(|| ir.plan.owner.clone());
            let inst = Instance {
                id: instance,
                rue_root,
            };
            match render(&ir.site, &ir.plan, &host, &inst, &bindings) {
                Ok(a) => {
                    out.write_all(a.text.as_bytes())?;
                    Ok(ExitCode::SUCCESS)
                }
                // A diagnostic or a refusal of the plan's own content is 1;
                // a wrong call (no backstop, an unknown host) is 2.
                Err(e) => {
                    eprintln!("rue: {e}");
                    Ok(match e {
                        RenderError::NoBackstop
                        | RenderError::NotTarget
                        | RenderError::UnknownHost(_) => ExitCode::from(2),
                        _ => ExitCode::from(1),
                    })
                }
            }
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    match run(cli, &mut lock) {
        Ok(code) => {
            let _ = lock.flush();
            code
        }
        Err(e) => {
            let _ = lock.flush();
            eprintln!("rue: {e:#}");
            ExitCode::from(2)
        }
    }
}
