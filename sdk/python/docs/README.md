# rue-hook for Python

A hook is a process that `rued` calls on a site's behalf: to record journal
entries, run steps on hosts it cannot reach itself, answer probes, approve
gates, resolve and receive secrets, deliver notifications, or schedule
backstops. The protocol is newline-delimited JSON ([hook-protocol.md]).
This package lets you write a hook as a few small classes, and does the
registration handshake, the framing and the reply shapes for you.

- **Standard library only.** Python 3.11 or later, and nothing else.
- **Conformance-tested.** `rue sdk-conform` drives every op of every kind
  through this package's own serve loop ([sdk-conformance.md]).
- **Protocol v1**, which is frozen: a hook written against it keeps working
  until a new protocol version says otherwise.

## Install

The package is `rue_hook`. It is not on PyPI; install it from a checkout
of rue:

```sh
pip install ./sdk/python
```

or put `sdk/python` on `PYTHONPATH`.

## Quick start

An audit hook: a journal sink that keeps every entry the engine chains,
and a notifier. This file is `examples/audit_hook.py`, and the package's
own tests run it.

<!-- example: examples/audit_hook.py -->
```python
#!/usr/bin/env python3
"""An audit hook: a journal sink that keeps every entry, and a notifier.

Bind it in a site with `journal to: local(), hook(:audit)` and
`notify via: hook(:audit)`, and have rued spawn it:

    rued run --spawn audit="python3 audit_hook.py" ...

Each journal entry is appended to $RUE_AUDIT_LOG (default audit.ndjson)
as one line of JSON. A sink that cannot record an entry must say so: the
engine then refuses to proceed (R0304) rather than run a step nobody
recorded. Notifications go to stderr, because stdout carries the protocol.
"""

import json
import os
import sys

from rue_hook import Hooks, Journal, Notify, Refusal, serve_stdio


class AuditLog(Journal):
    def __init__(self, path):
        self.path = path

    def append(self, entry):
        try:
            with open(self.path, "a", encoding="utf-8") as f:
                f.write(json.dumps(entry, sort_keys=True) + "\n")
        except OSError as e:
            raise Refusal(f"the audit log {self.path} is not writable: {e.strerror}")


class Stderr(Notify):
    def deliver(self, level, subject, body):
        print(f"[{level}] {subject}: {body}", file=sys.stderr, flush=True)


def hooks(path):
    return Hooks(journal=AuditLog(path), notify=Stderr())


if __name__ == "__main__":
    serve_stdio("audit", hooks(os.environ.get("RUE_AUDIT_LOG", "audit.ndjson")))
```

The protocol is plain lines, so you can drive the hook by hand. After its
registration line it waits for the acknowledgement, then answers one
request per line:

```text
$ printf '%s\n' '{"register":{"ok":true}}' \
    '{"id":1,"kind":"journal","op":"append","entry":{"seq":1}}' \
    '{"id":2,"kind":"probe","op":"observe","host":"h","probe":"p"}' |
  python3 examples/audit_hook.py
{"register": {"name": "audit", "kinds": ["journal", "notify"], "protocol": 1, "filesystem": false, "stdin_preamble": false}}
{"id": 1, "ok": true}
{"id": 2, "ok": false, "error": "this hook does not serve probe.observe"}
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
  --spawn audit="python3 /usr/local/libexec/audit_hook.py"
```

Four names have to agree: the one the hook registers with (the first
argument of `serve_stdio`), the `NAME` of `--spawn NAME=COMMAND`, the
`hook(:audit)` the site binds, and one in a registrar's `may_register`.
`rued` refuses a child that registers under any other name. A child `rued` spawned is the socket
owner, so its registrar says `user: :socket_owner`. A hook the journal or
the inventory depends on must be spawned this way: the daemon needs it
before it starts listening, so it cannot be one that connects later.

## Next

- [guide.md](guide.md): every kind and its handler, refusing, secrets,
  the budget, and serving over the control socket.
- [testing.md](testing.md): testing a hook, and judging it with
  `rue sdk-conform`.

[hook-protocol.md]: ../../../docs/hook-protocol.md
[sdk-conformance.md]: ../../../docs/sdk-conformance.md
