# rue_hook for Elixir

A hook is a process that `rued` calls on a site's behalf: to record journal
entries, run steps on hosts it cannot reach itself, answer probes, approve
gates, resolve and receive secrets, deliver notifications, or schedule
backstops. The protocol is newline-delimited JSON ([hook-protocol.md]).
This package lets you write a hook as a module per kind, does the
registration handshake, the framing and the reply shapes, and gives an
embedding host one process (`RueHook.Client`) that is a hook, an operator
and a subscriber on a single connection.

- **No dependencies.** Elixir 1.18 or later, whose standard library has
  `JSON`.
- **Conformance-tested.** `rue sdk-conform` drives every op of every kind
  through this package's own serve loop ([sdk-conformance.md]).
- **Protocol v1**, which is frozen: a hook written against it keeps working
  until a new protocol version says otherwise.

## Install

The application is `:rue_hook`. It is not on Hex; depend on it from a
checkout of rue:

```elixir
defp deps do
  [{:rue_hook, path: "../rue/sdk/elixir"}]
end
```

## Quick start

An audit hook: a journal sink that keeps every entry the engine chains,
and a notifier. This file is `examples/audit_hook.exs`; the package's own
tests run it as `rued` would (`test/audit_example_test.exs`).

<!-- example: examples/audit_hook.exs -->
```elixir
# An audit hook: a journal sink that keeps every entry, and a notifier.
#
# Bind it in a site with `journal to: local(), hook(:audit)` and
# `notify via: hook(:audit)`, and have rued spawn it:
#
#     rued run --spawn audit="elixir -pa _build/prod/lib/rue_hook/ebin audit_hook.exs" ...
#
# Each journal entry is appended to $RUE_AUDIT_LOG (default audit.ndjson)
# as one line of JSON. A sink that cannot record an entry must say so: the
# engine then refuses to proceed (R0304) rather than run a step nobody
# recorded. Notifications go to stderr, because stdout carries the protocol.

defmodule AuditHook.Log do
  def append(entry) do
    path = System.get_env("RUE_AUDIT_LOG", "audit.ndjson")

    case File.write(path, JSON.encode!(entry) <> "\n", [:append]) do
      :ok ->
        :ok

      {:error, why} ->
        {:refuse, "the audit log #{path} is not writable: #{:file.format_error(why)}"}
    end
  end
end

defmodule AuditHook.Stderr do
  def deliver(level, subject, body) do
    IO.puts(:stderr, "[#{level}] #{subject}: #{body}")
    :ok
  end
end

RueHook.Serve.stdio("audit", %RueHook.Hooks{journal: AuditHook.Log, notify: AuditHook.Stderr})
```

The protocol is plain lines, so you can drive the hook by hand. After its
registration line it waits for the acknowledgement, then answers one
request per line:

```text
$ mix compile
$ printf '%s\n' '{"register":{"ok":true}}' \
    '{"id":1,"kind":"journal","op":"append","entry":{"seq":1}}' \
    '{"id":2,"kind":"probe","op":"observe","host":"h","probe":"p"}' |
  elixir -pa _build/dev/lib/rue_hook/ebin examples/audit_hook.exs
{"register":{"filesystem":false,"kinds":["journal","notify"],"name":"audit","protocol":1,"stdin_preamble":false}}
{"id":1,"ok":true}
{"error":"this hook does not serve probe.observe","id":2,"ok":false}
```

## Wire it into a site

The site names the hook where it wants it used, and declares who may
register it:

```text
site do
  journal to: local(), hook(:audit)
  notify via: hook(:audit)
  hooks do
    registrar :spawned, user: :socket_owner, may_register: [:audit]
  end
  ...
end
```

Then `rued` starts it as a child and talks to it over its stdin and
stdout:

```sh
rued run --site site.rue --store /var/db/rue --socket /var/run/rue/rued.sock \
  --spawn audit="elixir -pa /opt/rue_hook/ebin /usr/local/libexec/audit_hook.exs"
```

Four names have to agree: the one the hook registers with (the first
argument of `RueHook.Serve.stdio/3`), the `NAME` of `--spawn NAME=COMMAND`,
the `hook(:audit)` the site binds, and one in a registrar's `may_register`.
`rued` refuses a child that registers under any other name. A child `rued`
spawned is the socket owner, so its registrar says `user: :socket_owner`. A
hook the journal or the inventory depends on must be spawned this way: the
daemon needs it before it starts listening, so it cannot be one that
connects later.

## Next

- [guide.md](guide.md): every kind and its handler, refusing, secrets,
  the budget, and `RueHook.Client` for a host that is also an operator.
- [testing.md](testing.md): testing a hook, and judging it with
  `rue sdk-conform`.

[hook-protocol.md]: ../../../docs/hook-protocol.md
[sdk-conformance.md]: ../../../docs/sdk-conformance.md
