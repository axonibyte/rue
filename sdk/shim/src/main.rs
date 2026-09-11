//! `rue-hook`: the shim of docs/ROADMAP.md 7.11.
//!
//! It registers as a hook and hands each request to a command, on that
//! command's stdin, reading the reply from its stdout. So a hook can be a
//! shell script, an awk program, a curl invocation -- anything that reads
//! a line and writes one -- without linking an SDK or speaking the
//! handshake.
//!
//! ```sh
//! rued run --spawn fence="rue-hook --name fence --kinds probe,execute \
//!                            --command ./fence.sh"
//! ```
//!
//! What the shim is responsible for, so the command is not:
//!
//! * the registration frame and the acknowledgement;
//! * the `id`, which it copies from the request onto the reply, so a
//!   command never has to parse one out or echo it back;
//! * a command that fails -- a non-zero exit, output that is not one JSON
//!   object -- becoming `ok: false` with the reason, rather than a silence
//!   the engine can only report as "the step did not happen".
//!
//! What the shim deliberately does **not** do is repair a reply. It passes
//! the command's own object through, `ok` and fields and all. A shim that
//! quietly completed a malformed reply would make the command's mistakes
//! invisible and would make `rue sdk-conform`'s provocations
//! inexpressible; the engine's R0303 is the honest answer to a reply that
//! promises what it does not carry.
//!
//! **Silence is expressible**: a command that exits 0 having written
//! nothing is taken to have chosen not to answer, and the shim writes no
//! reply. The engine reads that as Silent, which is a refusal of the step.
//! Because a command that merely *forgot* to print looks identical, the
//! shim says on its own stderr each time this happens, so the mistake is
//! visible to whoever reads the daemon's log.

use std::io::{BufReader, Read, Write};
use std::process::{Command, Stdio};

use clap::Parser;
use rue_hook_proto::{Registration, HOOK_PROTOCOL};
use rue_hook_sdk::proto::KINDS;
use rue_hook_sdk::{read_frame, refusal_frame, write_frame, Refusal};
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(
    name = "rue-hook",
    version,
    about = "Serve a rue hook by handing each request to a command."
)]
struct Args {
    /// The name to register as. Over stdio it must equal the name the
    /// daemon spawned this child under.
    #[arg(long)]
    name: String,
    /// The kinds this hook serves, comma separated (docs/ROADMAP.md 7.5).
    #[arg(long, value_delimiter = ',')]
    kinds: Vec<String>,
    /// The command each request is handed to, run by the host's shell with
    /// the request as one JSON line on its stdin.
    #[arg(long)]
    command: String,
    /// This hook serves the instance-directory ops (7.7). Declare it only
    /// when the command answers all of them.
    #[arg(long)]
    filesystem: bool,
    /// `env:` and `stdin:` reach the command through a preamble on stdin.
    #[arg(long)]
    stdin_preamble: bool,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    if args.kinds.is_empty() {
        eprintln!(
            "rue-hook: --kinds names at least one of: {}",
            KINDS.join(", ")
        );
        std::process::exit(2);
    }
    if let Some(bad) = args.kinds.iter().find(|k| !KINDS.contains(&k.as_str())) {
        eprintln!(
            "rue-hook: `{bad}` is not a kind of this protocol; the kinds are {}",
            KINDS.join(", ")
        );
        std::process::exit(2);
    }

    let stdin = std::io::stdin();
    let mut r = BufReader::new(stdin.lock());
    let mut w = std::io::stdout();

    let registration = Registration {
        name: args.name.clone(),
        kinds: args.kinds.clone(),
        protocol: HOOK_PROTOCOL,
        filesystem: args.filesystem,
        stdin_preamble: args.stdin_preamble,
    };
    write_frame(&mut w, &json!({ "register": registration }))?;
    match read_frame(&mut r)? {
        Some(ack) if ack.pointer("/register/ok") == Some(&json!(true)) => {}
        other => {
            eprintln!("rue-hook: registration was not acknowledged: {other:?}");
            std::process::exit(1);
        }
    }

    loop {
        let frame = match read_frame(&mut r) {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            // A line that is not JSON is skipped like any other line that
            // is not a request; returned as an error, it ended the shim.
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => continue,
            Err(e) => return Err(e),
        };
        if frame.is_null() || frame.get("kind").is_none() {
            continue;
        }
        let id = frame.get("id").cloned().unwrap_or(Value::Null);
        match hand_to_command(&args.command, &frame) {
            // The command chose not to answer.
            Answer::Silent => {
                eprintln!(
                    "rue-hook: {} wrote nothing and exited 0 for {}.{}: staying silent, which \
                     the engine reads as a refusal of the step",
                    args.command,
                    frame["kind"].as_str().unwrap_or("?"),
                    frame["op"].as_str().unwrap_or("?")
                );
            }
            Answer::Reply(mut reply) => {
                // The id is the shim's to set: a command never has to
                // parse one out, and one that echoes the wrong id would
                // be answering somebody else's question.
                reply["id"] = id;
                write_frame(&mut w, &reply)?;
            }
            Answer::Broken(why) => write_frame(&mut w, &refusal_frame(id, &Refusal::new(why)))?,
        }
    }
    Ok(())
}

enum Answer {
    Reply(Value),
    Silent,
    Broken(String),
}

/// Run the command with the request on its stdin and read one JSON object
/// from its stdout.
fn hand_to_command(command: &str, request: &Value) -> Answer {
    let (shell, flag) = rue_hook_sdk::proto::host_shell();
    let mut child = match Command::new(shell)
        .arg(flag)
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Answer::Broken(format!("cannot run `{command}`: {e}")),
    };
    if let Some(mut sink) = child.stdin.take() {
        let mut line = match serde_json::to_vec(request) {
            Ok(v) => v,
            Err(e) => return Answer::Broken(format!("cannot encode the request: {e}")),
        };
        line.push(b'\n');
        // A command that reads nothing closes its stdin, and writing to it
        // then fails; that is its business, not a fault of its own.
        let _ = sink.write_all(&line);
        let _ = sink.flush();
    }
    let mut out = String::new();
    if let Some(mut o) = child.stdout.take() {
        if let Err(e) = o.read_to_string(&mut out) {
            return Answer::Broken(format!("cannot read what `{command}` wrote: {e}"));
        }
    }
    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => return Answer::Broken(format!("`{command}` could not be waited for: {e}")),
    };
    let text = out.trim();
    if text.is_empty() {
        return if status.success() {
            Answer::Silent
        } else {
            Answer::Broken(format!(
                "`{command}` wrote no reply and exited {}",
                status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "on a signal".into())
            ))
        };
    }
    // The last line, so a command may write progress before its answer.
    let last = text.lines().next_back().unwrap_or(text);
    match serde_json::from_str::<Value>(last) {
        Ok(v) if v.is_object() => Answer::Reply(v),
        Ok(_) => Answer::Broken(format!(
            "`{command}` wrote `{last}`, which is not a JSON object"
        )),
        Err(e) => Answer::Broken(format!(
            "`{command}` wrote `{last}`, which is not JSON: {e}"
        )),
    }
}
