//! The two ways `rued` reaches a hook (docs/hook-protocol.md): as a child
//! it spawned, over the child's stdio, and as a client of the control
//! socket, after a `hello`.
//!
//! Both do the same three things: send the registration frame, read the
//! acknowledgement, then answer one request per line until the far end
//! closes. Over the socket there is a `hello` first, and event frames from
//! any subscription arrive interleaved with requests -- they are skipped
//! here, because a hook that is not also an operator has nothing to do
//! with them.

use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::{read_frame, refusal_frame, registration, write_frame, Hooks, Refusal};

/// How the serve loop behaves.
pub struct ServeOptions {
    /// The name this hook registers as. Over stdio it must equal the name
    /// the daemon spawned it under (`--spawn NAME=COMMAND`), and over the
    /// socket it must be one the connecting account may register (R0505).
    pub name: String,
    /// The identity to `hello` with; socket transport only. `None` lets
    /// the daemon name the peer from its credentials.
    pub identity: Option<String>,
    /// A budget for one handler. A handler that overruns it answers
    /// `ok: false` naming the overrun rather than leaving the engine to
    /// time the hook out, because a refusal with a reason is worth more to
    /// the operator than a silence. `None` leaves a slow handler to
    /// whatever the engine's own deadline decides.
    pub budget: Option<Duration>,
}

impl ServeOptions {
    pub fn new(name: &str) -> ServeOptions {
        ServeOptions {
            name: name.to_string(),
            identity: None,
            budget: None,
        }
    }
}

/// Serve on a spawned child's stdio: the registration frame is the first
/// line of stdout, before anything else, and the acknowledgement is the
/// first line of stdin. Anything else the process writes to stdout is a
/// protocol violation, so keep your own logging on stderr.
pub fn serve_stdio(hooks: Hooks, opts: ServeOptions) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut r = BufReader::new(stdin.lock());
    let mut w = std::io::stdout();
    handshake(&mut r, &mut w, &hooks, &opts, false)?;
    pump(&mut r, &mut w, hooks, &opts)
}

/// Serve over the control socket: `hello`, then `register`, then the same
/// loop. `stream` is anything that reads and writes the channel; a
/// `UnixStream` and its `try_clone` are the usual pair.
pub fn serve_socket<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    hooks: Hooks,
    opts: ServeOptions,
) -> std::io::Result<()> {
    handshake(reader, writer, &hooks, &opts, true)?;
    pump(reader, writer, hooks, &opts)
}

fn handshake<R: BufRead, W: Write>(
    r: &mut R,
    w: &mut W,
    hooks: &Hooks,
    opts: &ServeOptions,
    hello: bool,
) -> std::io::Result<()> {
    if hello {
        let mut h = json!({ "id": 0, "verb": "hello", "proto": 1 });
        if let Some(id) = &opts.identity {
            h["identity"] = json!(id);
        }
        write_frame(w, &h)?;
        let reply = expect_frame(r, "the daemon's hello reply")?;
        if reply.pointer("/result/ok") != Some(&json!(true))
            && reply.get("ok") != Some(&json!(true))
        {
            return Err(protocol(format!("the daemon refused the hello: {reply}")));
        }
    }
    write_frame(w, &json!({ "register": registration(&opts.name, hooks) }))?;
    let ack = expect_frame(r, "the registration acknowledgement")?;
    if ack.pointer("/register/ok") == Some(&json!(true)) {
        return Ok(());
    }
    Err(protocol(format!("registration was refused: {ack}")))
}

fn pump<R: BufRead, W: Write>(
    r: &mut R,
    w: &mut W,
    mut hooks: Hooks,
    opts: &ServeOptions,
) -> std::io::Result<()> {
    while let Some(frame) = read_frame(r)? {
        if frame.is_null() {
            continue;
        }
        // A subscription's events share the channel with requests; a hook
        // that is not also an operator has nothing to do with them.
        if frame.get("event").is_some() {
            continue;
        }
        if frame.get("kind").is_none() {
            continue;
        }
        let started = Instant::now();
        let mut reply = hooks.answer(&frame);
        if let Some(budget) = opts.budget {
            let took = started.elapsed();
            if took > budget && reply.get("ok") == Some(&json!(true)) {
                let id = frame.get("id").cloned().unwrap_or(Value::Null);
                reply = refusal_frame(
                    id,
                    &Refusal::new(format!(
                        "the handler took {}ms, over its {}ms budget; answering late is worse \
                         than answering no",
                        took.as_millis(),
                        budget.as_millis()
                    )),
                );
            }
        }
        write_frame(w, &reply)?;
    }
    Ok(())
}

fn expect_frame<R: BufRead>(r: &mut R, what: &str) -> std::io::Result<Value> {
    match read_frame(r)? {
        Some(v) if !v.is_null() => Ok(v),
        _ => Err(protocol(format!("the connection closed before {what}"))),
    }
}

fn protocol(m: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, m)
}
