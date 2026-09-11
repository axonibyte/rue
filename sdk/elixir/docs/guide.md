# Writing a hook in Elixir

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
  returns `{:ok, value}` (or `:ok` where the op has nothing to say), and
  the SDK builds the reply from the op's own row of the protocol table, so
  it cannot leave out a required field (at the engine that would be R0303,
  and the step would be refused).
- **A refusal.** `ok: false` with a reason, which the engine journals and
  the operator reads. A handler returns `{:refuse, reason}`. A kind your
  hook does not serve is refused for you, naming it; a handler that raises
  is refused with the exception's module and message, a function a module
  does not define with its `UndefinedFunctionError`, and a return that is
  none of the above with a refusal saying so -- a refusal with a reason is
  better than a hook that died.
- **Silence.** No reply before the engine's deadline (`rued run
  --hook-deadline`, 30 seconds by default). The engine refuses the step
  and can say nothing about why. The budget below turns a slow handler
  into a refusal instead.

## The kinds

Set the fields of `%RueHook.Hooks{}` you serve to a module; it registers
exactly those. Each module defines the functions of its kind:

| kind | site line | functions and what they return |
|---|---|---|
| `journal` | `journal to: hook(:x)` | `append(entry) :: :ok` |
| `inventory` | `inventory from: hook(:x)` | `list() :: {:ok, [host]}`, each host a map as `inventory.toml` records it (`"name"`, `"address"`, `"os"`, `"roles"`, `"reach"`, ...) |
| `execute` | `execute via: hook(:x, transport: :t)` | `run(host, instance, body) :: {:ok, %{"stdout" => text, "outputs" => %{name => text}}}`; the instance-directory functions below |
| `probe` | served with `execute` | `observe(host, probe) :: {:ok, RueHook.observation(:yes \| :no \| :unknown, text)}` |
| `approval` | `approval via: hook(:x)` | `authenticators() :: {:ok, [%{"id" => id, "human" => bool}]}`, `challenge(instance, digest, scope, context) :: {:ok, text}`, `verify(instance, digest, scope, authenticator, proof) :: {:ok, {verified, reason}}` |
| `secrets` | `secrets from: hook(:x)`, `secrets deliver_to: [..., hook(:x)]` | `resolve(ref) :: {:ok, value}`, `deliver(instance, label, value) :: {:ok, {accepted, receipt}}` |
| `notify` | `notify via: hook(:x)` | `deliver(level, subject, body) :: :ok` |
| `scheduler` | `backstop scheduler: hook(:x)` | `install(host, artifact)`, `arm(host, artifact, deadline)`, `rearm(...)`, `disarm(host, artifact) :: :ok`; `present(host, artifact) :: {:ok, true \| false \| "unknown"}` |

Every function may return `{:refuse, reason}` instead. Notes on the ones
with sharp edges:

- **journal.** A sink that does not acknowledge an entry stops the plan
  (R0304), so refuse only when you really did not record it.
- **probe.** A hook's probe is reached through an `execute via:
  hook(...)` binding: the engine asks the executor that serves the host.
- **execute.** Beyond `run`, a hook that sets `filesystem: true` must serve
  the instance-directory contract: `read_fact(host, shape)`,
  `bootstrap_state(host)`, `clock(host)`, `instance_dir_create(host,
  instance)`, `instance_dir_remove(host, instance)`,
  `instance_dir_list(host)`, `put_file(host, instance, rel, content, mode)`,
  `replace_file(host, instance, rel, content)`, `get_file(host, instance,
  rel)`, `remove_file(host, instance, rel)` and `host_lock(host)`
  (docs/ROADMAP.md 7.7). Two answers there are not what they look like:
  `read_fact` returning `{:ok, nil}` means *no such file*, which is an
  answer, while `{:ok, ""}` is a file that exists and is empty; and
  refusing `clock` tells the engine no clock-skew probe is possible on that
  host, which it accepts.
- **approval.** Verify a proof against the `digest` and `scope` you were
  handed, never against a request you rebuilt: that binding is what makes
  a proof for one step useless for another.
- **secrets.** `{:ok, {false, receipt}}` from `deliver` declines a secret,
  and the engine offers it to the next acceptor. The receipt is what it
  journals instead of the value.
- **scheduler.** Never guess `present`: the engine reads `false` as
  "install it again"; answer `"unknown"` when you do not know.

Handlers are modules, not processes: a handler that needs configuration
reads it where it runs (the application environment, `System.get_env/2`),
as the quick start does.

## Secrets in an `execute.run` body

`run` is one of the four messages a secret may travel in (the others are
`secrets.deliver`, `secrets.resolve`'s answer and a run's secret outputs).
The body is the wire's list of primitives, one map each
(`%{"run" => %{"cmd" => ..., "env" => [[name, value], ...]}}`), with every
resolved value a `%RueHook.Resolved{text, secret}`.

A secret `Resolved` inspects as `#RueHook.Resolved<secret>`, and
interpolates and encodes to JSON as `"<secret>"`, so a body in a log line,
a crash report or a reply does not carry one. `RueHook.expose/1` is how a
value is read, named so that a use of a secret is visible in the code that
makes it. Send it somewhere safe -- a child's environment or stdin, a
mode-0600 file -- never onto a command line, where every user of the host
can read it.

```elixir
def run(host, _instance, [%{"run" => prim} | _]) do
  env = for [name, value] <- prim["env"] || [], do: {name, RueHook.expose(value)}

  case System.cmd("deploy", [host], env: env, stderr_to_stdout: true) do
    {out, 0} -> {:ok, %{"stdout" => out, "outputs" => %{}}}
    {out, status} -> {:refuse, "deploy exited #{status}: #{out}"}
  end
end
```

## A budget for slow handlers

`RueHook.Serve.stdio(name, hooks, budget_ms: 5_000)` gives each handler five
seconds (`RueHook.Client` takes the same `:budget_ms`). A handler that takes
longer still finishes, but its reply is replaced by a refusal naming the
overrun, so the engine reads a reason rather than a silence. Set the budget
below `--hook-deadline`; with none a slow handler is left to the engine's
deadline.

## A host that is also an operator: `RueHook.Client`

`RueHook.Serve.stdio/3` is for a child `rued` spawns. A long-running
application that connects to a daemon already running uses
`RueHook.Client`, a GenServer that owns one connection and is, on it, a
hook the engine calls, an operator issuing verbs, and a subscriber to its
plans:

```elixir
{:ok, c} =
  RueHook.Client.start_link(
    socket: "/var/run/rue/rued.sock",
    identity: "reactive_host",
    events_to: self(),
    budget_ms: 5_000
  )

:ok = RueHook.Client.register(c, "host_actuate", %RueHook.Hooks{execute: Deploy})
{:ok, result} = RueHook.Client.call(c, "apply", %{"ir" => ir, "params" => %{}})

receive do
  {:rue_event, entry} -> entry
end
```

Requests, replies and journal events arrive interleaved on the one
connection; the GenServer demultiplexes them, answers requests with the
registered hooks, and sends each event to `:events_to` as `{:rue_event,
entry}` -- none is dropped while a verb is in flight. The identity and the
registrar are matched against the connecting process's OS user, so their
`user:` names that account. A hook that connects this way registers after
the daemon has booted, so it cannot be the site's journal or inventory.
The verbs and their arguments are docs/control-protocol.md's.

## What the SDK does for you

- Registers exactly the kinds you set, with the protocol version.
- Skips what is not a request: blank lines, lines that are not a JSON
  object, subscription events, frames with no `kind`. Every request gets
  exactly one reply.
- Writes each reply as one line, and refuses by name a reply it cannot
  encode (a tuple, a pid) rather than ending the loop -- or, in
  `RueHook.Client`, the connection.
- Keeps stdout for the protocol. Your own output belongs on stderr.
