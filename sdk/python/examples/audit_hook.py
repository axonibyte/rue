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
