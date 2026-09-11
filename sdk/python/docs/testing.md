# Testing a hook

## Unit-test the handlers

`Hooks.answer(request)` is the whole dispatch: it takes one request frame
as a dict and returns the reply the serve loop would write. Tests need no
process and no daemon:

```python
import unittest

from audit_hook import hooks


class AuditTest(unittest.TestCase):
    def test_an_entry_it_cannot_record_is_refused(self):
        h = hooks("/nonexistent/audit.ndjson")
        reply = h.answer({"id": 1, "kind": "journal", "op": "append", "entry": {"seq": 1}})
        self.assertFalse(reply["ok"])
        self.assertIn("is not writable", reply["error"])
```

`tests/test_audit_example.py` in this package tests the quick start this
way. Test the refusals as carefully as the answers: a refusal's text is
what the operator reads when a plan stops.

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
$ rue sdk-conform --name audit "python3 examples/audit_hook.py"
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
a hook that is not `examples/conformance_hook.py` fails them by design;
they judge the SDK, not your hook's behavior.

## Run a daemon in dry-run mode

`rued run --dry-run` needs no executors and turns every apply into a
rehearsal, which makes it a safe way to see your hook registered and
journaling:

```sh
RUE_AUDIT_LOG=$PWD/audit.ndjson rued run --dry-run --site site.rue \
  --store ./store --socket $PWD/rued.sock --group "$(id -gn)" \
  --spawn audit="python3 examples/audit_hook.py"
```

The first line in `audit.ndjson` is the daemon's record of the hook
registering (`hook_registered`), delivered through the hook itself.

## The package's own tests

`sh sdk/test-all.sh python`, from the repository root, runs this package's
suite (`python3 -m unittest discover -s tests -t .` inside `sdk/python`),
and `sh sdk/conform-all.sh python` judges its conformance hook. Both run in
rue's pipeline and on its Ubuntu test guest.

[sdk-conformance.md]: ../../../docs/sdk-conformance.md
