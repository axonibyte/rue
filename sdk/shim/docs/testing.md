# Testing a command

## Run it on one request

The command reads one line and writes one, so a test is a pipe:

```sh
printf '%s\n' '{"entry":{"seq":1},"id":1,"kind":"journal","op":"append"}' |
  RUE_AUDIT_LOG=/tmp/audit.ndjson sh examples/audit.sh
```

prints `{"ok":true}` and appends the request to `/tmp/audit.ndjson`. Test
the refusals as carefully as the answers: a refusal's text is what the
operator reads when a plan stops.

## Drive it through the shim

The README shows a session through `rue-hook` itself: the registration
line, the acknowledgement, and one reply per request, each with the
request's id. This is how `rued` sees it. `tests/audit_example.rs` runs the
quick start behind the shim this way.

## Judge it with `rue sdk-conform`

`rue sdk-conform` starts a hook the way `rued` does, checks its
registration, and then sends every op of every kind it registered,
checking each reply against the protocol (the id comes back, `ok` is a
boolean, an `ok: true` carries every required field, and it arrives within
the deadline) and against the answer [sdk-conformance.md] scripts for it.
Point it at the shim with your command behind it:

```text
$ rue sdk-conform --name audit "rue-hook --name audit --kinds journal,notify --command 'sh examples/audit.sh'"
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
command of your own passes as it stands. The other kinds' cases expect the
answers of the conformance world (its hosts, its probes, its proofs);
`tests/fixtures/conformance-command.sh` answers them all, and a command
that does not fails those cases by design.

## Run a daemon in dry-run mode

`rued run --dry-run` needs no executors and turns every apply into a
rehearsal, which makes it a safe way to see your hook registered and
journaling:

```sh
RUE_AUDIT_LOG=$PWD/audit.ndjson rued run --dry-run --site site.rue \
  --store ./store --socket $PWD/rued.sock --group "$(id -gn)" \
  --spawn audit="rue-hook --name audit --kinds journal,notify --command 'sh examples/audit.sh'"
```

The first line in `audit.ndjson` is the daemon's record of the hook
registering (`hook_registered`), delivered through the hook itself.

## The shim's own tests

`cargo test -p rue-hook` runs the shim's tests: a POSIX `sh` script behind
it passing every conformance case (`tests/conform.rs`), and the quick start
(`tests/audit_example.rs`). They are ordinary workspace tests, so rue's gate
runs them everywhere it runs `cargo test`.

[sdk-conformance.md]: ../../../docs/sdk-conformance.md
