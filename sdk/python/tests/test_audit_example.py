"""examples/audit_hook.py, the quick start of docs/README.md, does what the
page says it does."""

import contextlib
import importlib.util
import io
import json
import os
import tempfile
import unittest
from pathlib import Path

_path = Path(__file__).resolve().parents[1] / "examples" / "audit_hook.py"
_spec = importlib.util.spec_from_file_location("audit_hook", _path)
audit_hook = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(audit_hook)


def _req(kind, op, **fields):
    return {"id": 1, "kind": kind, "op": op, **fields}


class AuditExampleTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.log = os.path.join(self.dir.name, "audit.ndjson")

    def test_it_registers_for_journal_and_notify(self):
        self.assertEqual(audit_hook.hooks(self.log).kinds(), ["journal", "notify"])

    def test_each_entry_is_appended_as_one_line(self):
        h = audit_hook.hooks(self.log)
        for n in (1, 2):
            reply = h.answer(_req("journal", "append", entry={"seq": n, "kind": "Applied"}))
            self.assertEqual(reply, {"id": 1, "ok": True})
        lines = Path(self.log).read_text(encoding="utf-8").splitlines()
        self.assertEqual([json.loads(l)["seq"] for l in lines], [1, 2])

    def test_an_entry_it_cannot_record_is_refused_with_the_reason(self):
        h = audit_hook.hooks(os.path.join(self.dir.name, "no-such-dir", "audit.ndjson"))
        reply = h.answer(_req("journal", "append", entry={"seq": 1}))
        self.assertFalse(reply["ok"])
        self.assertIn("is not writable", reply["error"])

    def test_a_notification_goes_to_stderr_and_is_acknowledged(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            reply = audit_hook.hooks(self.log).answer(
                _req("notify", "deliver", level="warn", subject="plan held", body="waiting for approval")
            )
        self.assertEqual(reply, {"id": 1, "ok": True})
        self.assertEqual(err.getvalue(), "[warn] plan held: waiting for approval\n")


if __name__ == "__main__":
    unittest.main()
