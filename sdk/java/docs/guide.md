# Writing a hook in Java

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
  the operator reads. Throw `new Hooks.Refusal("why")`. A kind or op your
  hook does not implement is refused for you, naming it; so is any other
  `RuntimeException` a handler throws (`SimpleName: message`), because a
  refusal with a reason is better than a hook that died. An `Error` -- a
  `StackOverflowError`, an `OutOfMemoryError` -- still ends the hook.
- **Silence.** No reply before the engine's deadline (`rued run
  --hook-deadline`, 30 seconds by default). The engine refuses the step
  and can say nothing about why. The budget below turns a slow handler
  into a refusal instead.

## The kinds

Set the public fields of `Hooks` you serve; it registers exactly those.
Each is an interface in `Hooks`; the ones with a single method take a
lambda, and a method with a default refuses as unserved until you override
it. Replies that are records are maps, and `Hooks` has a constructor for
each so a handler never builds one by hand.

| kind | site line | interface and methods |
|---|---|---|
| `journal` | `journal to: hook(:x)` | `Journal.append(Object entry)` |
| `inventory` | `inventory from: hook(:x)` | `Inventory.list()` returning a `List` of host maps as `inventory.toml` records them (`name`, `address`, `os`, `roles`, `reach`, ...) |
| `execute` | `execute via: hook(:x, transport: :t)` | `Execute.run(host, instance, List<Object> body)` returning `Hooks.output(stdout, outputs)`; the instance-directory methods below |
| `probe` | served with `execute` | `Probe.observe(host, probe)` returning `Hooks.observation("yes" \| "no" \| "unknown", text)` |
| `approval` | `approval via: hook(:x)` | `Approval.authenticators()` returning `Hooks.authenticator(id, human)`s, `challenge(instance, digest, scope, context)` returning the text, `verify(instance, digest, scope, authenticator, proof)` returning `Hooks.verdict(verified, reason)` |
| `secrets` | `secrets from: hook(:x)`, `secrets deliver_to: [..., hook(:x)]` | `Secrets.resolve(reference)` returning the value, `deliver(instance, label, value)` returning `Hooks.delivery(accepted, receipt)` |
| `notify` | `notify via: hook(:x)` | `Notify.deliver(level, subject, body)` |
| `scheduler` | `backstop scheduler: hook(:x)` | `Scheduler.install(host, artifact)`, `arm(host, artifact, deadline)`, `rearm(...)`, `disarm(host, artifact)`, `present(host, artifact)` returning `true`, `false` or `"unknown"` |

Notes on the ones with sharp edges:

- **journal.** A sink that does not acknowledge an entry stops the plan
  (R0304), so refuse only when you really did not record it.
- **probe.** A hook's probe is reached through an `execute via:
  hook(...)` binding: the engine asks the executor that serves the host.
- **execute.** Beyond `run`, a hook that sets `hooks.filesystem = true`
  must serve the instance-directory contract: `readFact`, `bootstrapState`,
  `clock`, `instanceDirCreate`, `instanceDirRemove`, `instanceDirList`,
  `putFile`, `replaceFile`, `getFile`, `removeFile` and `hostLock`
  (docs/ROADMAP.md 7.7). Two answers there are not what they look like:
  `readFact` returning `null` means *no such file*, which is an answer,
  while `""` is a file that exists and is empty; and refusing `clock` tells
  the engine no clock-skew probe is possible on that host, which it
  accepts.
- **approval.** Verify a proof against the `digest` and `scope` you were
  handed, never against a request you rebuilt: that binding is what makes
  a proof for one step useless for another.
- **secrets.** `Hooks.delivery(false, receipt)` declines a secret, and the
  engine offers it to the next acceptor. The receipt is what it journals
  instead of the value.
- **scheduler.** Never guess `present`: the engine reads `false` as
  "install it again"; answer `"unknown"` when you do not know.

## Secrets in an `execute.run` body

`run` is one of the four messages a secret may travel in (the others are
`secrets.deliver`, `secrets.resolve`'s answer and a run's secret outputs).
The body is the wire's list of primitives, one map each (`{"run": {"cmd":
..., "env": [[name, value], ...]}}`), with every resolved value a
`Resolved(text, secret)`.

A secret `Resolved` formats as `<secret>` -- through `toString`, string
concatenation, `String.format` and the library's JSON writer -- so a body
in a log line, an exception message or a reply does not carry one. Read a
value with `Hooks.expose(value)`, named so that a use of a secret is visible
in the code that makes it, and send it somewhere safe -- a child's
environment or stdin, a mode-0600 file -- never onto a command line, where
every user of the host can read it.

```java
import dev.rue.hook.Hooks;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Map;

class Deploy implements Hooks.Execute {
    @Override
    public Map<String, Object> run(String host, String instance, List<Object> body) {
        Map<?, ?> run = (Map<?, ?>) ((Map<?, ?>) body.get(0)).get("run");
        ProcessBuilder deploy = new ProcessBuilder("deploy", host).redirectErrorStream(true);
        for (Object pair : (List<?>) run.get("env")) {
            List<?> kv = (List<?>) pair;
            deploy.environment().put((String) kv.get(0), Hooks.expose(kv.get(1)));
        }
        try {
            Process p = deploy.start();
            String out = new String(p.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
            if (p.waitFor() != 0) {
                throw new Hooks.Refusal("deploy exited " + p.exitValue() + ": " + out);
            }
            return Hooks.output(out, Map.of());
        } catch (IOException e) {
            throw new Hooks.Refusal("deploy did not start: " + e.getMessage());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new Hooks.Refusal("interrupted while deploy ran");
        }
    }
}
```

## A budget for slow handlers

`Serve.stdio(name, hooks, Duration.ofSeconds(5))` gives each handler five
seconds. A handler that takes longer still finishes, but its reply is
replaced by a refusal naming the overrun, so the engine reads a reason
rather than a silence. Set the budget below `--hook-deadline`;
`Serve.stdio(name, hooks)` leaves a slow handler to the engine's deadline.

## Transports

This library serves over stdio, as a child `rued` spawns (`--spawn`). It
has no socket client: a Java process that must connect to a daemon already
running, rather than be spawned by it, speaks docs/control-protocol.md
itself, or runs its handlers behind a spawned child. `Serve.serve(in, out,
hooks, budget)` is the loop `stdio` runs after registering, over any reader
and stream, for a transport of your own.

## What the library does for you

- Registers exactly the kinds you set, with the protocol version.
- Skips what is not a request: blank lines, lines that are not a JSON
  object (including one nested deeper than any protocol frame), subscription
  events, frames with no `kind`. Every request gets exactly one reply.
- Writes each reply as one flushed line, and refuses by name a reply it
  cannot write (a NaN, a type JSON has no spelling for) rather than ending
  the loop.
- Keeps stdout for the protocol. Your own output belongs on stderr.
