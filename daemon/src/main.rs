//! rued: the standalone engine daemon (docs/ROADMAP.md section 7).
//!
//! This unit brings the store's migration verb (7.13): explicit,
//! dry-runnable, refused on a store another account owns or a schema newer
//! than this build, and recorded for the daemon's next start, which
//! journals `Migrated`. The daemon proper (the control channel, the reap
//! and heartbeat threads, `--dry-run`) arrives with the next unit.
//!
//! Exit codes: 0 done; 1 refused (R0502, ownership, a locked store); 2
//! usage.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rue_engine::store::{migrate, StoreError};

#[derive(Parser)]
#[command(
    name = "rued",
    version,
    about = "rue's engine daemon",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
    /// Migrate the instance store's schema to this build's, explicitly.
    /// Runs with the daemon stopped; refuses a store owned by another
    /// account or written by a newer rued.
    Migrate {
        /// The store directory.
        #[arg(long)]
        store: PathBuf,
        /// Report the steps and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.verb {
        Verb::Migrate { store, dry_run } => {
            let by = whoami();
            match migrate(&store, dry_run, &by) {
                Ok(m) => {
                    let verb = if m.dry_run {
                        "would migrate"
                    } else {
                        "migrated"
                    };
                    println!(
                        "rued: {verb} {} from schema {} to {}",
                        store.display(),
                        m.from,
                        m.to
                    );
                    for s in &m.steps {
                        println!("  {s}");
                    }
                    ExitCode::SUCCESS
                }
                Err(
                    e @ (StoreError::Schema(_)
                    | StoreError::NotOwned { .. }
                    | StoreError::Locked(_)),
                ) => {
                    eprintln!("rued: {e}");
                    ExitCode::from(1)
                }
                Err(e) => {
                    eprintln!("rued: {e}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

/// The account running the migration, for the `Migrated{by}` entry.
fn whoami() -> String {
    for var in ["USER", "LOGNAME", "USERNAME"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                return v;
            }
        }
    }
    "unknown".into()
}
