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
use std::io::{self, Read, Write};
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
        /// Print the plan IR this text resolves to, instead of a verdict.
        ///
        /// This is what an embedded host sends over the control channel to
        /// apply a plan (section 7.11): resolving `.rue` text needs the
        /// front end, the front end is Rust, and a host in another language
        /// therefore asks for the IR here rather than linking it.
        #[arg(long, conflicts_with = "json")]
        ir: bool,
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
    /// Judge a hook against the protocol (docs/sdk-conformance.md): an SDK
    /// passes this before it calls itself an SDK.
    SdkConform {
        /// The command that starts the hook, run with `sh -c`.
        command: String,
        /// The name it must register as.
        #[arg(long, default_value = "conform")]
        name: String,
        /// How long one reply may take before it counts as Silent.
        #[arg(long, default_value_t = 2000)]
        deadline_ms: u64,
        /// Print the report as JSON instead of a line per case.
        #[arg(long)]
        json: bool,
    },
    /// The journal: verify a chain end to end (section 5.10).
    Journal {
        #[command(subcommand)]
        verb: JournalVerb,
    },
    /// Apply a plan through the daemon (section 6.8): check, request,
    /// approve where no gate stands, run; the verdict line is last.
    Apply {
        /// A .rue file, or a plan IR document.
        plan: PathBuf,
        #[command(flatten)]
        select: Select,
        #[command(flatten)]
        channel: Channel,
        /// A plan parameter, `name=value`; repeatable.
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// Run under mode :auto or :manual, overriding the plan.
        #[arg(long)]
        mode: Option<String>,
        /// Acknowledge a knell up front: `N:"reason"`; repeatable.
        #[arg(long = "ack", value_name = "N:REASON")]
        ack: Vec<String>,
        /// Force an unknown guard by name; repeatable.
        #[arg(long = "force", value_name = "GUARD")]
        force: Vec<String>,
        /// A rehearsal against the real daemon: gates evaluated, every step
        /// journaled, nothing run, nothing reserved.
        #[arg(long)]
        dry_run: bool,
    },
    /// The observed state of one instance, or of every instance in scope.
    Status {
        instance: Option<String>,
        #[command(flatten)]
        channel: Channel,
    },
    /// Run the undo. Names are guards, or the classes `drift` and `unknown`.
    Recant {
        instance: String,
        #[arg(long = "force", value_delimiter = ',')]
        force: Vec<String>,
        #[command(flatten)]
        channel: Channel,
    },
    /// Fetch a secret a `hold()` acceptor kept, exactly once.
    Reveal {
        instance: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Submit a proof for a plan-entry or step gate; the token is read
    /// from stdin. With nothing on stdin the challenge is printed and
    /// nothing is submitted.
    Approve {
        instance: String,
        /// The step whose gate this proof is for; the plan gate otherwise.
        #[arg(long)]
        step: Option<u32>,
        /// The authenticator the proof is from; your own identity by
        /// default.
        #[arg(long)]
        authenticator: Option<String>,
        #[command(flatten)]
        channel: Channel,
    },
    /// Acknowledge a knell: a proof in the ack scope and a reason for the
    /// journal. The token is read from stdin.
    Ack {
        instance: String,
        #[arg(long)]
        step: u32,
        #[arg(long)]
        reason: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Extend a temporary plan; rearms the backstop first.
    Renew {
        instance: String,
        /// The new wane, e.g. 2h.
        #[arg(long)]
        wane: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Disarm an unless_confirmed backstop.
    Confirm {
        instance: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// End a permanent plan from Held or Deferred.
    Commit {
        instance: String,
        #[arg(long)]
        reason: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Continue a Held instance from its held step.
    Resume {
        instance: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Continue a Deferred instance after the handoff.
    HandoffDone {
        instance: String,
        #[arg(long)]
        step: u32,
        #[command(flatten)]
        channel: Channel,
    },
    /// Admin: close a Stuck or DriftHeld instance, the world left as it is.
    Abandon {
        instance: String,
        #[arg(long)]
        reason: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Cancel a pending request.
    Cancel {
        instance: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Admin: verify a target's rue_root; print the commands for what it
    /// lacks, never running them (section 7.7).
    Bootstrap {
        host: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Admin: remove an orphaned instance directory from a host (7.7).
    /// Refused while its artifact is armed and the scheduler entry is
    /// present (R0405); `--force` then needs a `--reason`.
    Reclaim {
        host: String,
        instance: String,
        /// Reclaim an armed artifact anyway, having read it.
        #[arg(long)]
        force: bool,
        /// Why, for the journal. Required with --force.
        #[arg(long, default_value = "")]
        reason: String,
        #[command(flatten)]
        channel: Channel,
    },
    /// Admin: bindings, executors, schedulers, bootstrap, sinks and modes,
    /// as the daemon sees them.
    Doctor {
        /// Also prove a real backstop fires: a throwaway artifact on every
        /// host with a scheduler, armed with a deadline already past, and
        /// removed whatever happens.
        #[arg(long)]
        canary: bool,
        /// How long to wait for a canary, in seconds.
        #[arg(long, default_value_t = 180)]
        canary_wait: u64,
        #[command(flatten)]
        channel: Channel,
    },
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

#[derive(Subcommand)]
enum JournalVerb {
    /// Verify a journal file (one JSON entry a line): genesis, sequence,
    /// every link and every hash; with --key, every signature too, and an
    /// unsigned entry is then a failure.
    Verify {
        /// The journal file.
        file: PathBuf,
        /// The public key (OpenSSH format) every entry must be signed with.
        #[arg(long)]
        key: Option<PathBuf>,
    },
}

/// How the daemon is reached (section 7.4).
#[derive(clap::Args, Default)]
struct Channel {
    /// The control socket; `RUE_SOCKET` when absent, else /var/run/rue/rued.sock.
    #[arg(long, env = "RUE_SOCKET", default_value = "/var/run/rue/rued.sock")]
    socket: PathBuf,
    /// The identity to connect as; the sole identity this user maps to
    /// when absent.
    #[arg(long = "identity")]
    identity: Option<String>,
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
    /// The inventory to check against, overriding the file the site names.
    /// Required for a site whose `inventory from:` is a hook: its hosts do
    /// not exist until the hook is asked, and checking needs them now
    /// (E0607).
    #[arg(long)]
    inventory: Option<PathBuf>,
}

/// A diagnostic on stderr: miette's report with the source line and a
/// caret when the diagnostic has a span in a readable file, else the
/// one-line rendering.
fn report(d: &rue_core::diagnostics::Diagnostic) {
    use miette::{
        GraphicalReportHandler, GraphicalTheme, LabeledSpan, MietteDiagnostic, NamedSource, Report,
    };
    let Some(span) = &d.span else {
        eprintln!("{}", d.render());
        return;
    };
    let Ok(src) = fs::read_to_string(&span.file) else {
        eprintln!("{}", d.render());
        return;
    };
    let offset: usize = src
        .lines()
        .take(span.line.saturating_sub(1) as usize)
        .map(|l| l.len() + 1)
        .sum::<usize>()
        + span.col.saturating_sub(1) as usize;
    let len = src[offset.min(src.len())..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ',' | ')' | ']' | '}'))
        .map(char::len_utf8)
        .sum::<usize>()
        .max(1);
    let mut message = d.message.clone();
    if let Some(e) = &d.expected {
        message.push_str(&format!("; expected {e}"));
    }
    if let Some(f) = &d.found {
        message.push_str(&format!(", found {f}"));
    }
    let mut diag = MietteDiagnostic::new(message)
        .with_code(d.code.to_string())
        .with_label(LabeledSpan::at(offset..offset + len, "here"));
    if let Some(n) = &d.nearest {
        diag = diag.with_help(format!("did you mean {n}?"));
    }
    let report = Report::new(diag).with_source_code(NamedSource::new(&span.file, src));
    let mut out = String::new();
    let _ = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
        .render_report(&mut out, report.as_ref());
    eprint!("{out}");
}

/// A plan IR document, or a `.rue` file resolved for one host. A resolver
/// diagnostic is a refusal (exit 1) with the diagnostics on stderr.
fn load_input(
    path: &PathBuf,
    select: &Select,
    host_on_ir: bool,
) -> Result<Result<PlanIr, ExitCode>> {
    load_input_opts(path, select, host_on_ir, false)
}

fn load_input_opts(
    path: &PathBuf,
    select: &Select,
    host_on_ir: bool,
    suspend_e0604: bool,
) -> Result<Result<PlanIr, ExitCode>> {
    if path.extension().is_some_and(|e| e == "rue") {
        let opts = rue_surface::resolve::Options {
            suspend_e0604,
            host: select.host.clone(),
            plan: select.plan_name.clone(),
            requester: select.requester.clone(),
            inventory: select.inventory.clone(),
        };
        return Ok(match rue_surface::resolve::resolve(path, &opts) {
            Ok(ir) => Ok(ir),
            Err(diags) => {
                for d in &diags {
                    report(d);
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
        Verb::Check {
            plan,
            json,
            ir: print_ir,
            select,
        } => {
            let ir = match load_input(&plan, &select, false)? {
                Ok(ir) => ir,
                Err(code) => return Ok(code),
            };
            if print_ir {
                // Canonically encoded, as the channel carries it: an
                // embedded host sends these bytes and the daemon checks
                // the same text this command just did.
                out.write_all(&canonical::encode(&serde_json::to_value(&ir)?)?)?;
                return Ok(ExitCode::SUCCESS);
            }
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
        Verb::Journal {
            verb: JournalVerb::Verify { file, key },
        } => {
            let entries =
                rue_engine::store::read_ndjson(&file).map_err(|e| anyhow::anyhow!("{e}"))?;
            if entries.is_empty() {
                eprintln!(
                    "rue: {}: no entries; a chain with nothing in it verifies nothing",
                    file.display()
                );
                return Ok(ExitCode::from(1));
            }
            let pk = match &key {
                Some(k) => {
                    Some(rue_engine::sign::load_public(k).map_err(|e| anyhow::anyhow!("{e}"))?)
                }
                None => None,
            };
            match rue_engine::sign::verify_chain(&entries, pk.as_ref()) {
                Ok(()) => {
                    let signed = match &key {
                        Some(k) => format!(", every signature verified with {}", k.display()),
                        None => String::new(),
                    };
                    writeln!(
                        out,
                        "rue: {}: {} entries, chain verified{signed}",
                        file.display(),
                        entries.len()
                    )?;
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("rue: {}: {e}", file.display());
                    Ok(ExitCode::from(1))
                }
            }
        }
        Verb::Apply {
            plan,
            select,
            channel,
            set,
            mode,
            ack,
            force,
            dry_run,
        } => {
            // The daemon's hello says whether it is in dry-run mode, which
            // decides whether a site with no operators block is admitted.
            let dry_run_daemon = match daemon_dry_run(&channel) {
                Ok(d) => d,
                Err(code) => return Ok(code),
            };
            let ir = match load_input_opts(&plan, &select, false, dry_run_daemon)? {
                Ok(ir) => ir,
                Err(code) => return Ok(code),
            };
            let mut params = serde_json::Map::new();
            for kv in &set {
                let (k, v) = kv
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("--set takes NAME=VALUE, not {kv}"))?;
                params.insert(k.to_string(), serde_json::Value::String(v.to_string()));
            }
            let mut acks = Vec::new();
            for a in &ack {
                let (n, _reason) = a
                    .split_once(':')
                    .ok_or_else(|| anyhow::anyhow!("--ack takes N:REASON, not {a}"))?;
                acks.push(n.parse::<u32>().with_context(|| format!("--ack {a}"))?);
            }
            let args = serde_json::json!({
                "ir": ir,
                "params": params,
                "acks": acks,
                "forced": force,
                "mode": mode,
                "rehearsal": dry_run,
            });
            over_channel(&channel, "apply", args, out)
        }
        Verb::Status { instance, channel } => over_channel(
            &channel,
            "status",
            serde_json::json!({ "instance": instance }),
            out,
        ),
        Verb::Recant {
            instance,
            force,
            channel,
        } => over_channel(
            &channel,
            "recant",
            serde_json::json!({ "instance": instance, "force": force }),
            out,
        ),
        Verb::Reveal { instance, channel } => over_channel(
            &channel,
            "reveal",
            serde_json::json!({ "instance": instance }),
            out,
        ),
        Verb::Approve {
            instance,
            step,
            authenticator,
            channel,
        } => {
            let proof = read_stdin()?;
            let mut args = serde_json::json!({ "instance": instance });
            if let Some(n) = step {
                args["step"] = serde_json::json!(n);
            }
            if proof.trim().is_empty() {
                return over_channel(&channel, "challenge", args, out);
            }
            args["proof"] = serde_json::json!(proof.trim());
            if let Some(a) = authenticator {
                args["authenticator"] = serde_json::json!(a);
            }
            over_channel(&channel, "approve", args, out)
        }
        Verb::Ack {
            instance,
            step,
            reason,
            channel,
        } => over_channel(
            &channel,
            "ack",
            serde_json::json!({
                "instance": instance,
                "step": step,
                "reason": reason,
                "proof": read_stdin()?.trim(),
            }),
            out,
        ),
        Verb::Renew {
            instance,
            wane,
            channel,
        } => {
            let secs = parse_duration(&wane)?;
            over_channel(
                &channel,
                "renew",
                serde_json::json!({ "instance": instance, "wane_s": secs }),
                out,
            )
        }
        Verb::Confirm { instance, channel } => over_channel(
            &channel,
            "confirm",
            serde_json::json!({ "instance": instance }),
            out,
        ),
        Verb::Commit {
            instance,
            reason,
            channel,
        } => over_channel(
            &channel,
            "commit",
            serde_json::json!({ "instance": instance, "reason": reason }),
            out,
        ),
        Verb::Resume { instance, channel } => over_channel(
            &channel,
            "resume",
            serde_json::json!({ "instance": instance }),
            out,
        ),
        Verb::HandoffDone {
            instance,
            step,
            channel,
        } => over_channel(
            &channel,
            "handoff_done",
            serde_json::json!({ "instance": instance, "step": step }),
            out,
        ),
        Verb::Abandon {
            instance,
            reason,
            channel,
        } => over_channel(
            &channel,
            "abandon",
            serde_json::json!({ "instance": instance, "reason": reason }),
            out,
        ),
        Verb::Cancel { instance, channel } => over_channel(
            &channel,
            "cancel",
            serde_json::json!({ "instance": instance }),
            out,
        ),
        Verb::Bootstrap { host, channel } => over_channel(
            &channel,
            "bootstrap",
            serde_json::json!({ "host": host }),
            out,
        ),
        Verb::Reclaim {
            host,
            instance,
            force,
            reason,
            channel,
        } => over_channel(
            &channel,
            "reclaim",
            serde_json::json!({
                "host": host,
                "instance": instance,
                "force": force,
                "reason": reason,
            }),
            out,
        ),
        Verb::Doctor {
            canary,
            canary_wait,
            channel,
        } => over_channel(
            &channel,
            "doctor",
            serde_json::json!({ "canary": canary, "canary_wait_s": canary_wait }),
            out,
        ),
        Verb::States => {
            out.write_all(render_table().as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        Verb::SdkConform {
            command,
            name,
            deadline_ms,
            json,
        } => {
            let report = match rue_engine::conform::conform(
                &name,
                &command,
                std::time::Duration::from_millis(deadline_ms),
            ) {
                Ok(r) => r,
                Err(e) => {
                    // Exit 2: the suite could not be run at all, which is a
                    // different thing from a hook that ran and failed.
                    eprintln!("rue sdk-conform: {e}");
                    return Ok(ExitCode::from(2));
                }
            };
            if json {
                writeln!(out, "{}", report.to_json())?;
            } else {
                for o in &report.outcomes {
                    writeln!(
                        out,
                        "{}  {} :: {}\n        {}",
                        if o.passed { "ok    " } else { "not ok" },
                        o.op,
                        o.case,
                        o.detail
                    )?;
                }
                writeln!(
                    out,
                    "\n{} passed, {} failed, against the protocol of docs/hook-protocol.md v1",
                    report.passed(),
                    report.failed()
                )?;
            }
            Ok(if report.failed() == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
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
                        report(d);
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

/// A verb over the channel: connect, hello, call; print the result's line
/// last on stdout; the exit code is the outcome's, or 2 for a contract,
/// identity or scope error, 1 for a refusal, 75 for R0101.
fn over_channel(
    ch: &Channel,
    verb: &str,
    args: serde_json::Value,
    out: &mut dyn Write,
) -> Result<ExitCode> {
    use rue_engine::control::Client;
    let mut c = Client::connect(&ch.socket)
        .with_context(|| format!("connecting to {} (is rued running?)", ch.socket.display()))?;
    if let Err(e) = c.hello(ch.identity.as_deref()) {
        eprintln!("rue: {e}");
        return Ok(ExitCode::from(2));
    }
    match c.call(verb, args) {
        Ok(result) => {
            if let Some(items) = result.as_array() {
                for r in items {
                    writeln!(out, "{}", status_line(r))?;
                }
                if items.is_empty() {
                    writeln!(out, "no instances")?;
                }
                return Ok(ExitCode::SUCCESS);
            }
            if let Some(cmds) = result.get("commands").and_then(|c| c.as_array()) {
                let host = result.get("host").and_then(|h| h.as_str()).unwrap_or("");
                let ready = result
                    .get("ready")
                    .and_then(|r| r.as_bool())
                    .unwrap_or(false);
                if ready {
                    writeln!(out, "{host}: bootstrapped")?;
                    return Ok(ExitCode::SUCCESS);
                }
                writeln!(out, "{host}: not bootstrapped; as root on {host}:")?;
                for c in cmds {
                    writeln!(out, "  {}", c.as_str().unwrap_or(""))?;
                }
                return Ok(ExitCode::from(1));
            }
            if let Some(report) = result.get("report") {
                let healthy = result
                    .get("healthy")
                    .and_then(|h| h.as_bool())
                    .unwrap_or(false);
                for h in report
                    .get("hosts")
                    .and_then(|h| h.as_array())
                    .into_iter()
                    .flatten()
                {
                    let name = h.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let ex = h
                        .get("executor")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unreachable");
                    let boot = match h.get("bootstrap").and_then(|b| b.as_object()) {
                        Some(b) => {
                            let ok = ["rue_root", "group", "instances_dir", "lock", "modes_ok"]
                                .iter()
                                .all(|k| b.get(*k).and_then(|v| v.as_bool()) == Some(true));
                            if ok {
                                "bootstrapped"
                            } else {
                                "not bootstrapped"
                            }
                        }
                        None => "bootstrap unknown",
                    };
                    let sched = h
                        .get("scheduler")
                        .and_then(|v| v.as_str())
                        .unwrap_or("no scheduler");
                    writeln!(out, "host {name}: {ex}, {boot}, {sched}")?;
                }
                let sinks: Vec<String> = report
                    .get("sinks")
                    .and_then(|s| s.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                writeln!(
                    out,
                    "journal: {} sink(s) ({}), {}",
                    sinks.len(),
                    sinks.join(", "),
                    if report.get("signed").and_then(|v| v.as_bool()) == Some(true) {
                        "signed"
                    } else {
                        "unsigned"
                    }
                )?;
                let hooks: Vec<String> = result
                    .get("hooks")
                    .and_then(|s| s.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                writeln!(
                    out,
                    "hooks registered: {}",
                    if hooks.is_empty() {
                        "none".to_string()
                    } else {
                        hooks.join(", ")
                    }
                )?;
                writeln!(
                    out,
                    "instances: {}{}",
                    report
                        .get("instances")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    if report.get("settling").and_then(|v| v.as_bool()) == Some(true) {
                        " (settling)"
                    } else {
                        ""
                    }
                )?;
                // A canary is the one line that says a real backstop
                // fired on a real host, and how long it took.
                for c in result
                    .get("canaries")
                    .and_then(|c| c.as_array())
                    .into_iter()
                    .flatten()
                {
                    let host = c.get("host").and_then(|v| v.as_str()).unwrap_or("");
                    let sched = c.get("scheduler").and_then(|v| v.as_str()).unwrap_or("");
                    let fired = c.get("fired").and_then(|v| v.as_bool()) == Some(true);
                    let note = c.get("note").and_then(|v| v.as_str()).unwrap_or("");
                    if fired {
                        writeln!(
                            out,
                            "canary {host}: {sched} fired it after {}s",
                            c.get("after_s").and_then(|v| v.as_u64()).unwrap_or(0)
                        )?;
                    } else {
                        writeln!(out, "canary {host}: {sched} fired nothing ({note})")?;
                    }
                }
                writeln!(
                    out,
                    "{}",
                    if healthy {
                        "doctor: healthy"
                    } else {
                        "doctor: attention needed"
                    }
                )?;
                return Ok(ExitCode::from(if healthy { 0 } else { 1 }));
            }
            // A challenge is the one reply that is a message for a
            // person: what the approval binding wants signed.
            if let Some(c) = result.get("challenge").and_then(|c| c.as_str()) {
                writeln!(out, "{c}")?;
                return Ok(ExitCode::SUCCESS);
            }
            // A revealed secret is the other: its value, once, and only to
            // the client that asked (5.13).
            if let (Some(label), Some(value)) = (
                result.get("label").and_then(|l| l.as_str()),
                result.get("value").and_then(|v| v.as_str()),
            ) {
                writeln!(out, "{label}={value}")?;
                return Ok(ExitCode::SUCCESS);
            }
            if result.get("line").is_some() {
                let line = result.get("line").and_then(|l| l.as_str()).unwrap_or("");
                let exit = result.get("exit").and_then(|e| e.as_u64()).unwrap_or(2) as u8;
                writeln!(out, "{line}")?;
                return Ok(ExitCode::from(exit));
            }
            writeln!(out, "{}", status_line(&result))?;
            let exit = result.get("exit").and_then(|e| e.as_u64()).unwrap_or(0) as u8;
            Ok(ExitCode::from(exit))
        }
        Err(e) => {
            eprintln!("rue: {e}");
            Ok(ExitCode::from(match e.code.as_str() {
                "R0101" => 75,
                "refused" => 1,
                _ => 2,
            }))
        }
    }
}

/// Ask the daemon whether it runs in dry-run mode (a hello and nothing
/// else); a connection or identity failure is reported as the verb would.
fn daemon_dry_run(ch: &Channel) -> Result<bool, ExitCode> {
    use rue_engine::control::Client;
    let mut c = match Client::connect(&ch.socket) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "rue: connecting to {} (is rued running?): {e}",
                ch.socket.display()
            );
            return Err(ExitCode::from(2));
        }
    };
    match c.hello(ch.identity.as_deref()) {
        Ok(h) => Ok(h.dry_run),
        Err(e) => {
            eprintln!("rue: {e}");
            Err(ExitCode::from(2))
        }
    }
}

fn status_line(r: &serde_json::Value) -> String {
    let s = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut line = format!(
        "{}: {} ({} on {})",
        s("id"),
        s("state").to_lowercase(),
        s("plan"),
        s("owner")
    );
    if let Some(d) = r.get("deadline").and_then(|v| v.as_u64()) {
        line.push_str(&format!(", wane at {d}"));
    }
    if let Some(w) = r.get("waiting").and_then(|v| v.as_object()) {
        line.push_str(&format!(
            ", waiting at step {} ({})",
            w.get("step").and_then(|v| v.as_u64()).unwrap_or(0),
            w.get("reason").and_then(|v| v.as_str()).unwrap_or("")
        ));
    }
    if let Some(h) = r.get("held_at").and_then(|v| v.as_u64()) {
        line.push_str(&format!(", held at step {h}"));
    }
    if let Some(st) = r.get("stuck").and_then(|v| v.as_array()) {
        if !st.is_empty() {
            line.push_str(&format!(", stuck at {st:?}"));
        }
    }
    if r.get("rehearsal").and_then(|v| v.as_bool()) == Some(true) {
        line.push_str(", rehearsal");
    }
    line
}

/// `2h`, `30m`, `90s`, `1d` to seconds.
fn parse_duration(s: &str) -> Result<u64> {
    let (num, unit) = s.split_at(s.trim_end_matches(|c: char| c.is_ascii_alphabetic()).len());
    let n: u64 = num.parse().with_context(|| format!("duration {s}"))?;
    Ok(match unit {
        "s" | "" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        other => anyhow::bail!("duration unit {other} in {s}"),
    })
}

/// A proof or token on stdin. Nothing there is not an error: `rue
/// approve` with no token prints the challenge instead of submitting.
fn read_stdin() -> anyhow::Result<String> {
    use std::io::IsTerminal;
    if io::stdin().is_terminal() {
        return Ok(String::new());
    }
    let mut s = String::new();
    io::stdin().read_to_string(&mut s)?;
    Ok(s)
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
