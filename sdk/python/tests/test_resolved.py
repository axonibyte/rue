"""7.11: a run's secrets reach the handler, and cannot be formatted onto a
command line or into a log by accident."""

import json
import unittest

from rue_hook import Execute, Hooks, Resolved, RPrim

PW = "correct-horse-battery"


class ResolvedTest(unittest.TestCase):
    def test_a_secret_is_redacted_wherever_it_is_formatted(self):
        r = Resolved(PW, True)
        for shown in (repr(r), str(r), f"{r}", "%s" % (r,), "{}".format(r), str([r]), str({"k": r})):
            self.assertNotIn(PW, shown)
        self.assertEqual(r.text, PW, "the field is how it is read")

    def test_a_run_handler_receives_resolved_values_and_a_careless_format_carries_no_secret(self):
        seen = []

        class E(Execute):
            def run(self, host, instance, body):
                seen.extend(body)
                return {"stdout": "", "outputs": {}}

        wire = [{"run": {"cmd": {"text": "deploy", "secret": False},
                         "env": [["PW", {"text": PW, "secret": True}]]}}]
        r = Hooks(execute=E()).answer({"id": 1, "kind": "execute", "op": "run",
                                       "host": "h", "instance": "i", "body": wire})
        self.assertTrue(r["ok"], r)
        self.assertNotIn(PW, str(seen))
        prim = seen[0]
        self.assertIsInstance(prim, RPrim)
        self.assertTrue(prim.carries_secret())
        self.assertEqual(dict(prim.fields["env"])["PW"].text, PW)
        self.assertEqual(prim.fields["cmd"].text, "deploy")

    def test_a_secret_a_handler_puts_in_its_reply_is_not_written_out(self):
        # A handler that echoes a body value into its outputs by mistake:
        # the reply is still written, and not with the secret's text.
        from rue_hook.serve import _write
        import io

        out = io.StringIO()
        _write(out, {"id": 1, "ok": True, "output": {"stdout": "", "outputs": {"x": Resolved(PW, True)}}})
        self.assertNotIn(PW, out.getvalue())
        json.loads(out.getvalue())


if __name__ == "__main__":
    unittest.main()
