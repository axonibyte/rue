# Writing a hook in Python

## How the engine talks to a hook

The engine sends one request per line and waits for one reply per request,
carrying the request's `id`:

```text
engine → hook  {"id": 7, "kind": "probe", "op": "observe", "host": "db-01", "probe": "fenced"}
hook → engine  {"id": 7, "ok": true, "fact": {"text": "fenced at 02:10", "tri": "yes"}}
hook → engine  {"id": 7, "ok": false, "error": "the fence controller is unreachable"}
```

There are three ways to answer, and the SDK makes the fourth unreachable:

- **An answer.** `ok: true` with every field the op requires. The SDK
  builds the reply from the op's own row of the protocol table, so a
  handler cannot leave out a required field (at the engine that would be
  R0303, and the step would be refused).
- **A refusal.** `ok: false` with a reason, which the engine journals and
  the operator reads. Raise `Refusal("why")`. An op your hook does not
  implement is refused for you, naming it; so is any other exception a
  handler raises (`TypeName: message`), because a refusal with a reason is
  better than a hook that died.
- **Silence.** No reply before the engine's deadline (`rued run
  --hook-deadline`, 30 seconds by default). The engine refuses the step
  and can say nothing about why. The budget below turns a slow handler
  into a refusal instead.

## The kinds

Fill in the kinds you serve; `Hooks` registers exactly those. Each is a
class to subclass, and a method you do not override refuses as unserved.

| kind | site line | class and methods |
|---|---|---|
| `journal` | `journal to: hook(:x)` | `Journal.append(entry: dict) -> None` |
| `inventory` | `inventory from: hook(:x)` | `Inventory.list() -> list[dict]`, one dict per host as `inventory.toml` records it (`name`, `address`, `os`, `roles`, `reach`, ...) |
| `execute` | `execute via: hook(:x, transport: :t)` | `Execute.run(host, instance, body: list[RPrim]) -> dict` returning `{"stdout": str, "outputs": {name: str}}`; the instance-directory methods below |
| `probe` | served with `execute` | `Probe.observe(host, probe) -> Observation` |
| `approval` | `approval via: hook(:x)` | `Approval.authenticators() -> list[Authenticator]`, `challenge(ChallengeRequest) -> str`, `verify(VerifyRequest) -> Verdict` |
| `secrets` | `secrets from: hook(:x)`, `secrets deliver_to: [..., hook(:x)]` | `Secrets.resolve(reference) -> str`, `deliver(instance, label, value) -> Delivery` |
| `notify` | `notify via: hook(:x)` | `Notify.deliver(level, subject, body) -> None` |
| `scheduler` | `backstop scheduler: hook(:x)` | `Scheduler.install(host, artifact)`, `arm(host, artifact, deadline)`, `rearm(...)`, `disarm(host, artifact)`, `present(host, artifact)` |

Notes on the ones with sharp edges:

- **journal.** A sink that does not acknowledge an entry stops the plan
  (R0304), so refuse only when you really did not record it.
- **probe.** `Observation.yes(text)`, `.no(text)` or `.unknown(text)`. A
  hook's probe is reached through an `execute via: hook(...)` binding: the
  engine asks the executor that serves the host.
- **execute.** Beyond `run`, a hook that registers with
  `Hooks(filesystem=True)` must serve the instance-directory contract:
  `read_fact`, `bootstrap_state`, `clock`, `instance_dir_create`,
  `instance_dir_remove`, `instance_dir_list`, `put_file`, `replace_file`,
  `get_file`, `remove_file` and `host_lock` (docs/ROADMAP.md 7.7). Two
  answers there are not what they look like: `read_fact` returning `None`
  means *no such file*, which is an answer, while `""` is a file that exists
  and is empty; and refusing `clock` tells the engine no clock-skew probe is
  possible on that host, which it accepts.
- **approval.** Verify a proof against the `digest` and `scope` you were
  handed, never against a request you rebuilt: that binding is what makes
  a proof for one step useless for another.
- **secrets.** `Delivery(False, "")` declines a secret, and the engine
  offers it to the next acceptor. The receipt is what it journals instead
  of the value.
- **scheduler.** `present` returns `Presence.PRESENT`, `Presence.ABSENT` or
  `Presence.UNKNOWN`. Never guess: the engine reads *absent* as "install it
  again".

## Secrets in an `execute.run` body

`run` is one of the four messages a secret may travel in (the others are
`secrets.deliver`, `secrets.resolve`'s answer and a run's secret outputs).
The body is a list of `RPrim`, one per primitive: `prim.name` is the
primitive (`run`, `write`, `region_set`, ...) and `prim.fields` its values,
each resolved value a `Resolved(text, secret)`. The `env` field is a list of
`(name, Resolved)` pairs.

A `Resolved` holding a secret prints as `<secret>` through `repr`, and so
through `str`, f-strings, logging and tracebacks. Its value is `.text`:
reach for it only where it is going somewhere safe, such as a child's stdin
or environment, or a mode-0600 file, and never onto a command line, where
every user of the host can read it. `prim.carries_secret()` tells you
whether a primitive holds one.

```python
import os
import subprocess

def run(self, host, instance, body):
    prim = body[0]
    env = dict(os.environ)
    env.update((name, value.text) for name, value in prim.fields.get("env", []))
    done = subprocess.run(["deploy", host], env=env, capture_output=True, text=True)
    return {"stdout": done.stdout, "outputs": {}}
```

## A budget for slow handlers

`serve_stdio(name, hooks, budget=5.0)` gives each handler five seconds. A
handler that takes longer still finishes, but its reply is replaced by a
refusal naming the overrun, so the engine reads a reason rather than a
silence. Set the budget below `--hook-deadline`; with no budget a slow
handler is left to the engine's deadline.

## Serving over the control socket

`serve_stdio` is for a child `rued` spawns. A long-running process that
connects to a daemon already running uses `serve_socket`:

```python
from rue_hook import serve_socket

serve_socket("/var/run/rue/rued.sock", "audit", hooks, identity=None, budget=5.0)
```

It sends `hello`, then `register`, then answers requests on the same
connection. The registrar that admits the name is matched against the
connecting process's OS user, so its `user:` names that account rather than
`:socket_owner`. A hook that connects this way registers after the daemon
has booted, so it cannot be the site's journal or inventory.

## What the SDK does for you

- Registers exactly the kinds you filled in, with the protocol version.
- Skips what is not a request: blank lines, lines that are not a JSON
  object, subscription events, frames with no `kind`. Every request gets
  exactly one reply.
- Writes each reply as one flushed line, and refuses by name a reply it
  cannot write (a set, a NaN, an object `json` does not know) rather than
  ending the loop.
- Keeps stdout for the protocol. Your own output belongs on stderr.
