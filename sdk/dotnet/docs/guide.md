# Writing a hook in .NET

## How the engine talks to a hook

The engine sends one request per line and waits for one reply per request,
carrying the request's `id`:

```text
engine → hook  {"id": 7, "kind": "probe", "op": "observe", "host": "db-01", "probe": "fenced"}
hook → engine  {"id": 7, "ok": true, "fact": {"text": "fenced at 02:10", "tri": "yes"}}
hook → engine  {"id": 7, "ok": false, "error": "the fence controller is unreachable"}
```

There are three ways to answer, and the library makes the fourth
unreachable:

- **An answer.** `ok: true` with every field the op requires. A handler
  returns its value, and the library builds the reply from the op's own
  row of the protocol table, so it cannot leave out a required field (at
  the engine that would be R0303, and the step would be refused).
- **A refusal.** `ok: false` with a reason, which the engine journals and
  the operator reads. Throw `new Refusal("why")`. A kind or op your hook
  does not implement is refused for you, naming it; so is any other
  exception a handler throws (`TypeName: message`), and a reply the
  serializer cannot write, because a refusal with a reason is better than a
  hook that died.
- **Silence.** No reply before the engine's deadline (`rued run
  --hook-deadline`, 30 seconds by default). The engine refuses the step
  and can say nothing about why. The budget below turns a slow handler
  into a refusal instead.

## The kinds

Set the properties of `Hooks` you serve; it registers exactly those. Each
is an interface, and a method with a default implementation refuses as
unserved until you implement it. Replies that are records are built with
`Hooks`'s constructors, so a handler never assembles one by hand.

| kind | site line | interface and methods |
|---|---|---|
| `journal` | `journal to: hook(:x)` | `IJournal.Append(JsonNode entry)` |
| `inventory` | `inventory from: hook(:x)` | `IInventory.List()` returning host dictionaries as `inventory.toml` records them (`name`, `address`, `os`, `roles`, `reach`, ...) |
| `execute` | `execute via: hook(:x, transport: :t)` | `IExecute.Run(host, instance, IReadOnlyList<JsonNode?> body)` returning `Hooks.Output(stdout, outputs)`; the instance-directory methods below |
| `probe` | served with `execute` | `IProbe.Observe(host, probe)` returning `Hooks.Observation("yes" \| "no" \| "unknown", text)` |
| `approval` | `approval via: hook(:x)` | `IApproval.Authenticators()` returning `Hooks.Authenticator(id, human)`s, `Challenge(instance, digest, scope, context)` returning the text, `Verify(instance, digest, scope, authenticator, proof)` returning `Hooks.Verdict(verified, reason)` |
| `secrets` | `secrets from: hook(:x)`, `secrets deliver_to: [..., hook(:x)]` | `ISecrets.Resolve(reference)` returning the value, `Deliver(instance, label, value)` returning `Hooks.Delivery(accepted, receipt)` |
| `notify` | `notify via: hook(:x)` | `INotify.Deliver(level, subject, body)` |
| `scheduler` | `backstop scheduler: hook(:x)` | `IScheduler.Install(host, artifact)`, `Arm(host, artifact, deadline)`, `Rearm(...)`, `Disarm(host, artifact)`, `Present(host, artifact)` returning `true`, `false` or `"unknown"` |

Notes on the ones with sharp edges:

- **journal.** A sink that does not acknowledge an entry stops the plan
  (R0304), so refuse only when you really did not record it.
- **probe.** A hook's probe is reached through an `execute via:
  hook(...)` binding: the engine asks the executor that serves the host.
- **execute.** Beyond `Run`, a hook that sets `Filesystem = true` must serve
  the instance-directory contract: `ReadFact`, `BootstrapState`, `Clock`,
  `InstanceDirCreate`, `InstanceDirRemove`, `InstanceDirList`, `PutFile`,
  `ReplaceFile`, `GetFile`, `RemoveFile` and `HostLock` (docs/ROADMAP.md
  7.7). Two answers there are not what they look like: `ReadFact` returning
  `null` means *no such file*, which is an answer, while `""` is a file that
  exists and is empty; and refusing `Clock` tells the engine no clock-skew
  probe is possible on that host, which it accepts.
- **approval.** Verify a proof against the `digest` and `scope` you were
  handed, never against a request you rebuilt: that binding is what makes
  a proof for one step useless for another.
- **secrets.** `Hooks.Delivery(false, receipt)` declines a secret, and the
  engine offers it to the next acceptor. The receipt is what it journals
  instead of the value.
- **scheduler.** Never guess `Present`: the engine reads `false` as
  "install it again"; answer `"unknown"` when you do not know.

## Secrets in an `execute.run` body

`run` is one of the four messages a secret may travel in (the others are
`secrets.deliver`, `secrets.resolve`'s answer and a run's secret outputs).
The body is the wire's list of primitives, one `JsonObject` each
(`{"run": {"cmd": ..., "env": [[name, value], ...]}}`), with every resolved
value a `JsonValue` wrapping a `Resolved(Text, Secret)`.

A secret formats as `<secret>` -- through `ToString`, interpolation,
`ToJsonString` and `JsonSerializer` -- so a body in a log line, an exception
message or a reply does not carry one. Read a value with
`Hooks.Expose(node)`, named so that a use of a secret is visible in the
code that makes it, and send it somewhere safe -- a child's environment or
stdin, a mode-0600 file -- never onto a command line, where every user of
the host can read it.

```csharp
using System.Diagnostics;
using System.Text.Json.Nodes;
using Rue.Hook;

sealed class Deploy : IExecute
{
    public object Run(string host, string instance, IReadOnlyList<JsonNode?> body)
    {
        var run = body[0]?["run"] ?? throw new Refusal("this hook runs one `run` primitive");
        var start = new ProcessStartInfo("deploy", host) { RedirectStandardOutput = true };
        if (run["env"] is JsonArray env)
        {
            foreach (var pair in env.OfType<JsonArray>())
            {
                start.Environment[pair[0]!.GetValue<string>()] = Hooks.Expose(pair[1]);
            }
        }

        using var deploy = Process.Start(start) ?? throw new Refusal("deploy did not start");
        var stdout = deploy.StandardOutput.ReadToEnd();
        deploy.WaitForExit();
        if (deploy.ExitCode != 0)
        {
            throw new Refusal($"deploy exited {deploy.ExitCode}");
        }

        return Hooks.Output(stdout, new Dictionary<string, object?>());
    }
}
```

## A budget for slow handlers

`Serve.Stdio(name, hooks, TimeSpan.FromSeconds(5))` gives each handler five
seconds. A handler that takes longer still finishes, but its reply is
replaced by a refusal naming the overrun, so the engine reads a reason
rather than a silence. Set the budget below `--hook-deadline`; with none a
slow handler is left to the engine's deadline.

## Transports

This library serves over stdio, as a child `rued` spawns (`--spawn`). It
has no socket client: a .NET process that must connect to a daemon already
running, rather than be spawned by it, speaks docs/control-protocol.md
itself, or runs its handlers behind a spawned child. `Serve.Loop(input,
output, hooks, budget)` is the loop `Stdio` runs after registering, over
any `TextReader` and `TextWriter`, for a transport of your own. `Stdio`
reads and writes UTF-8 whatever the console's encoding says.

## What the library does for you

- Registers exactly the kinds you set, with the protocol version.
- Skips what is not a request: blank lines, lines that are not a JSON
  object, subscription events, frames with no `kind`. Every request gets
  exactly one reply.
- Writes each reply as one flushed line.
- Keeps stdout for the protocol. Your own output belongs on stderr.
