# Testing a hook

## Unit-test the handlers

`Hooks::answer(&request)` is the whole dispatch: it takes one request frame
as a `serde_json::Value` and returns the reply the serve loop would write.
Tests need no process and no daemon:

```rust
use serde_json::json;

#[test]
fn an_entry_it_cannot_record_is_refused() {
    let mut hooks = audit_hook::hooks("/nonexistent/audit.ndjson".into());
    let reply = hooks.answer(&json!({
        "id": 1, "kind": "journal", "op": "append", "entry": { "seq": 1 }
    }));
    assert_eq!(reply["ok"], json!(false));
    assert!(reply["error"].as_str().unwrap().contains("is not writable"));
}
```

`tests/audit_example.rs` in this crate tests the quick start this way.
Test the refusals as carefully as the answers: a refusal's text is what the
operator reads when a plan stops.

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
$ rue sdk-conform --name audit target/debug/examples/audit_hook
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
a hook that is not `src/bin/conform_hook.rs` fails them by design; they
judge the SDK, not your hook's behavior.

## Run a daemon in dry-run mode

`rued run --dry-run` needs no executors and turns every apply into a
rehearsal, which makes it a safe way to see your hook registered and
journaling:

```sh
RUE_AUDIT_LOG=$PWD/audit.ndjson rued run --dry-run --site site.rue \
  --store ./store --socket $PWD/rued.sock --group "$(id -gn)" \
  --spawn audit=target/debug/examples/audit_hook
```

The first line in `audit.ndjson` is the daemon's record of the hook
registering (`hook_registered`), delivered through the hook itself.

## The crate's own tests

`cargo test -p rue-hook-sdk` runs this crate's tests, including its
conformance run (`tests/conform.rs`, which judges `rue-conform-hook` with
the same runner `rue sdk-conform` uses). They are ordinary workspace tests,
so rue's gate runs them everywhere it runs `cargo test`.

[sdk-conformance.md]: ../../../docs/sdk-conformance.md
