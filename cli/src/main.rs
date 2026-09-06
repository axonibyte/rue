//! `rue`, the operator CLI (docs/ROADMAP.md section 6.8), as far as Phase 1's
//! first unit takes it: `check` and `explain` over a plan IR document, and
//! `states`. The surface verbs (`.rue` input, `--host`, `--as`) arrive with
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
    /// Check a plan IR document and print its verdict.
    Check {
        /// The plan IR (docs/TESTING.md, "The plan IR").
        plan: PathBuf,
        /// Print the structured verdict in canonical JSON instead of the prose.
        #[arg(long)]
        json: bool,
    },
    /// List a plan's numbered steps with their undo lines, loci and policies.
    Explain {
        /// The plan IR.
        plan: PathBuf,
    },
    /// Print the runtime state machine's transition table.
    States,
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
        Verb::Check { plan, json } => {
            let ir = load(&plan)?;
            let v = verdict_of(&ir);
            if json {
                out.write_all(&canonical::encode(&to_json(&v))?)?;
            } else {
                out.write_all(prose(&v).as_bytes())?;
            }
            Ok(status_code(&v))
        }
        Verb::Explain { plan } => {
            let ir = load(&plan)?;
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
