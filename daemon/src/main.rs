//! rued: the standalone engine daemon (docs/ROADMAP.md section 7).
//!
//! `rued run` reads a site block (7.3, 7.4), opens the store, binds the
//! control socket and serves it (docs/control-protocol.md), reaps on an
//! interval (7.8), and hosts the hooks that register (docs/hook-protocol.md).
//! `rued migrate` is the store's migration (7.13). Daemon dry-run mode
//! (`--dry-run`, 7.9) registers no executors, suspends E0604, and makes
//! every apply a rehearsal.
//!
//! Exit codes: 0 done; 1 refused (a site the daemon cannot start on, R0502,
//! a locked store); 2 usage.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rue_engine::store::{migrate, StoreError};

#[cfg(unix)]
mod run;

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
    /// Serve the control socket over a site until stopped.
    Run {
        /// The .rue file whose site block declares the bindings, operators
        /// and registrars (a plan file or a file that only holds a site).
        #[arg(long)]
        site: PathBuf,
        /// The store directory; created with this build's schema when empty.
        #[arg(long)]
        store: PathBuf,
        /// The control socket path.
        #[arg(long)]
        socket: PathBuf,
        /// The group the socket belongs to (a name or a numeric gid);
        /// mode 0660.
        #[arg(long, default_value = "rue")]
        group: String,
        /// Daemon dry-run mode (7.9): no executors, E0604 suspended,
        /// every apply a rehearsal.
        #[arg(long)]
        dry_run: bool,
        /// Seconds between reap passes.
        #[arg(long, default_value_t = 5)]
        reap_every: u64,
        /// Seconds a hook has to answer a request.
        #[arg(long, default_value_t = 30)]
        hook_deadline: u64,
        /// Spawn a hook child over stdio: NAME=COMMAND; repeatable. The
        /// child registers as the socket owner and must be a declared
        /// registrar's hook.
        #[arg(long = "spawn", value_name = "NAME=COMMAND")]
        spawn: Vec<String>,
    },
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
        #[cfg(unix)]
        Verb::Run {
            site,
            store,
            socket,
            group,
            dry_run,
            reap_every,
            hook_deadline,
            spawn,
        } => match run::run(run::Config {
            site,
            store,
            socket,
            group,
            dry_run,
            reap_every,
            hook_deadline,
            spawn,
        }) {
            Ok(()) => ExitCode::SUCCESS,
            Err(run::Refusal::Usage(m)) => {
                eprintln!("rued: {m}");
                ExitCode::from(2)
            }
            Err(run::Refusal::Refused(m)) => {
                eprintln!("rued: {m}");
                ExitCode::from(1)
            }
        },
        #[cfg(not(unix))]
        Verb::Run { .. } => {
            eprintln!("rued: the control channel on Windows (a named pipe with a group DACL) arrives with the Windows unit; run is refused here");
            ExitCode::from(2)
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
