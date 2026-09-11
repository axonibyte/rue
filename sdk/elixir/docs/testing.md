# Testing a hook

## Unit-test the handlers

`RueHook.Hooks.answer/2` is the whole dispatch: it takes one request frame
as a map and returns the reply the serve loop would write. Tests need no
process and no daemon:

```elixir
test "a notification is acknowledged" do
  hooks = %RueHook.Hooks{notify: AuditHook.Stderr}
  request = %{"id" => 1, "kind" => "notify", "op" => "deliver",
              "level" => "warn", "subject" => "s", "body" => "b"}
  assert RueHook.Hooks.answer(hooks, request) == %{"id" => 1, "ok" => true}
end
```

Test the refusals as carefully as the answers: a refusal's text is what the
operator reads when a plan stops. The quick start is an `.exs` script that
starts serving as soon as it loads, so this package's
`test/audit_example_test.exs` runs it the other way -- as a child spoken to
through a `Port`, exactly as `rued` runs it. Keep the handler modules of a
real hook in `lib/`, and they can be tested directly as above.

## Drive it over stdio

A hook is a process reading lines, so the shell can drive one: its first
line is the registration, it waits for `{"register":{"ok":true}}`, and it
then answers each request line. The README shows a session. Your own output
must go to stderr; anything else on stdout is a line the engine cannot
read.

## Judge it with `rue sdk-conform`

`rue sdk-conform` starts a hook the way `rued` does, checks its
registration, and then sends every op of every kind it registered,
checking each reply against the protocol (the id comes back, `ok` is a
boolean, an `ok: true` carries every required field, and it arrives within
the deadline) and against the answer [sdk-conformance.md] scripts for it:

```text
$ rue sdk-conform --name audit "elixir -pa _build/dev/lib/rue_hook/ebin examples/audit_hook.exs"
ok      registration :: the first line is a registration this protocol admits
        serves journal, notify
ok      journal.append :: an entry is acknowledged
        acknowledged
ok      notify.deliver :: a notification is acknowledged
        acknowledged
ok      execute.reboot :: an op this protocol has no row for is refused, never met with silence
        refused by name

4 passed, 0 failed, against the protocol of docs/hook-protocol.md v1
```

For `journal` and `notify` the scripted answer is an acknowledgement, so a
hook of your own passes as it stands. The other kinds' cases expect the
answers of the conformance world (its hosts, its probes, its proofs), and
a hook that is not `examples/conformance_hook.exs` fails them by design;
they judge the SDK, not your hook's behavior.

## Run a daemon in dry-run mode

`rued run --dry-run` needs no executors and turns every apply into a
rehearsal, which makes it a safe way to see your hook registered and
journaling:

```sh
RUE_AUDIT_LOG=$PWD/audit.ndjson rued run --dry-run --site site.rue \
  --store ./store --socket $PWD/rued.sock --group "$(id -gn)" \
  --spawn audit="elixir -pa _build/dev/lib/rue_hook/ebin examples/audit_hook.exs"
```

The first line in `audit.ndjson` is the daemon's record of the hook
registering (`hook_registered`), delivered through the hook itself.

## The package's own tests

`sh sdk/test-all.sh elixir`, from the repository root, runs this package's
suite (`mix test --warnings-as-errors`, after a warnings-as-errors compile),
and `sh sdk/conform-all.sh elixir` judges its conformance hook. Both run in
rue's pipeline and on its Ubuntu test guest.

[sdk-conformance.md]: ../../../docs/sdk-conformance.md
