# Writing a hook in Rust

## How the engine talks to a hook

The engine sends one request per line and waits for one reply per request,
carrying the request's `id`:

```text
engine → hook  {"id": 7, "kind": "probe", "op": "observe", "host": "db-01", "probe": "fenced"}
hook → engine  {"id": 7, "ok": true, "fact": {"text": "fenced at 02:10", "tri": "yes"}}
hook → engine  {"id": 7, "ok": false, "error": "the fence controller is unreachable"}
```

There are three ways to answer, and the SDK makes the fourth unreachable:

- **An answer.** `ok: true` with every field the op requires. A handler
  returns `Ok(record)`, and the SDK builds the reply from the op's own row
  of the protocol table, so it cannot leave out a required field (at the
  engine that would be R0303, and the step would be refused).
- **A refusal.** `ok: false` with a reason, which the engine journals and
  the operator reads. A handler returns `Err(Refusal::new("why"))`; every
  handler's type is `Answer<T>`, which is `Result<T, Refusal>`. An op your
  hook does not implement is refused for you, naming it.
- **Silence.** No reply before the engine's deadline (`rued run
  --hook-deadline`, 30 seconds by default). The engine refuses the step
  and can say nothing about why. The budget below turns a slow handler
  into a refusal instead.

A handler that panics ends the hook process, and the engine sees a hook
that has gone: its requests fall silent until it is spawned again. Return
a `Refusal` instead; a reason is worth more to the operator than a crash.

## The kinds

Set the fields of `Hooks` you serve; it registers exactly those. Each is a
trait, and a method with a default refuses as unserved until you override
it. The records are `rue_hook_sdk::proto`'s.

| kind | site line | trait and methods |
|---|---|---|
| `journal` | `journal to: hook(:x)` | `Journal::append(&mut self, entry: &Value) -> Answer<()>` |
| `inventory` | `inventory from: hook(:x)` | `Inventory::list(&mut self) -> Answer<Vec<InventoryHost>>` |
| `execute` | `execute via: hook(:x, transport: :t)` | `Execute::run(&mut self, host, instance, body: &[RPrim]) -> Answer<Output>`; the instance-directory methods below |
| `probe` | served with `execute` | `Probe::observe(&mut self, host, probe) -> Answer<Observation>` |
| `approval` | `approval via: hook(:x)` | `Approval::authenticators() -> Answer<Vec<Authenticator>>`, `challenge(&ChallengeRequest) -> Answer<String>`, `verify(&VerifyRequest) -> Answer<Verdict>` |
| `secrets` | `secrets from: hook(:x)`, `secrets deliver_to: [..., hook(:x)]` | `Secrets::resolve(&mut self, reference) -> Answer<String>`, `deliver(instance, label, value) -> Answer<Delivery>` |
| `notify` | `notify via: hook(:x)` | `Notify::deliver(&mut self, level, subject, body) -> Answer<()>` |
| `scheduler` | `backstop scheduler: hook(:x)` | `Scheduler::install(host, artifact)`, `arm(host, artifact, deadline: Option<u64>)`, `rearm(...)`, `disarm(host, artifact)`, `present(host, artifact) -> Answer<Presence>` |

Notes on the ones with sharp edges:

- **journal.** A sink that does not acknowledge an entry stops the plan
  (R0304), so refuse only when you really did not record it.
- **probe.** `Observation::yes(text)`, `::no(text)` or `::unknown(text)`.
  A hook's probe is reached through an `execute via: hook(...)` binding: the
  engine asks the executor that serves the host.
- **execute.** Beyond `run`, a hook that sets `hooks.filesystem = true`
  must serve the instance-directory contract: `read_fact`,
  `bootstrap_state`, `clock`, `instance_dir_create`, `instance_dir_remove`,
  `instance_dir_list`, `put_file`, `replace_file`, `get_file`,
  `remove_file` and `host_lock` (docs/ROADMAP.md 7.7). Two answers there are
  not what they look like: `read_fact` returning `Ok(None)` means *no such
  file*, which is an answer, while `Ok(Some(String::new()))` is a file that
  exists and is empty; and refusing `clock` tells the engine no clock-skew
  probe is possible on that host, which it accepts.
- **approval.** Verify a proof against the `digest` and `scope` you were
  handed, never against a request you rebuilt: that binding is what makes
  a proof for one step useless for another.
- **secrets.** `Delivery { accepted: false, .. }` declines a secret, and the
  engine offers it to the next acceptor. The receipt is what it journals
  instead of the value.
- **scheduler.** `present` answers `Presence::Present`, `Absent` or
  `Unknown`. Never guess: the engine reads *absent* as "install it again".

## Secrets in an `execute.run` body

`run` is one of the four messages a secret may travel in (the others are
`secrets.deliver`, `secrets.resolve`'s answer and a run's secret outputs).
The body is a slice of `RPrim`, one variant per primitive (`Run { cmd, env,
stdin }`, `Write { shape, content }`, `RegionSet { .. }`, ...), each resolved
value a `Resolved { text, secret }`.

`Resolved` has no `Display`, and its `Debug` prints a secret as
`<secret>`, so a body in a log line, a panic message or a failed assertion
does not carry one. Read a value with `expose(&value)`, named so that a use
of a secret is visible in the code that makes it, and send it somewhere
safe -- a child's stdin or environment, a mode-0600 file -- never onto a
command line, where every user of the host can read it.
`carries_secret(body)` tells you whether a body holds one. (`Resolved` is
`Serialize`, because that is how it crosses the wire: serializing one
writes its text.)

```rust
use std::process::Command;

use rue_hook_sdk::proto::{Output, RPrim};
use rue_hook_sdk::{expose, Answer, Execute, Refusal};

struct Deploy;

impl Execute for Deploy {
    fn run(&mut self, host: &str, _instance: &str, body: &[RPrim]) -> Answer<Output> {
        let Some(RPrim::Run { env, .. }) = body.first() else {
            return Err(Refusal::new("this hook runs one `run` primitive"));
        };
        let mut deploy = Command::new("deploy");
        deploy.arg(host);
        for (name, value) in env {
            deploy.env(name, expose(value));
        }
        let done = deploy
            .output()
            .map_err(|e| Refusal::new(format!("deploy did not start: {e}")))?;
        Ok(Output {
            stdout: String::from_utf8_lossy(&done.stdout).into_owned(),
            ..Output::default()
        })
    }
}
```

## A budget for slow handlers

`ServeOptions { budget: Some(Duration::from_secs(5)), .. }` gives each
handler five seconds. A handler that takes longer still finishes, but its
reply is replaced by a refusal naming the overrun, so the engine reads a
reason rather than a silence. Set the budget below `--hook-deadline`; with
`None` a slow handler is left to the engine's deadline.

## Serving over the control socket

`serve_stdio` is for a child `rued` spawns. A long-running process that
connects to a daemon already running uses `serve_socket`, over any reader
and writer of the channel:

```rust
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use rue_hook_sdk::{serve_socket, Hooks, ServeOptions};

fn serve(hooks: Hooks) -> std::io::Result<()> {
    let stream = UnixStream::connect("/var/run/rue/rued.sock")?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut opts = ServeOptions::new("audit");
    opts.budget = Some(Duration::from_secs(5));
    serve_socket(&mut reader, &mut writer, hooks, opts)
}
```

It sends `hello` (with `opts.identity`, when set), then `register`, then
answers requests on the same connection. The registrar that admits the name
is matched against the connecting process's OS user, so its `user:` names
that account rather than `:socket_owner`. A hook that connects this way
registers after the daemon has booted, so it cannot be the site's journal
or inventory.

## What the SDK does for you

- Registers exactly the kinds you set, with the protocol version.
- Skips what is not a request: blank lines, lines that are not a JSON
  object, subscription events, frames with no `kind`. Every request gets
  exactly one reply.
- Writes each reply as one flushed line.
- Keeps stdout for the protocol. Your own output belongs on stderr.
