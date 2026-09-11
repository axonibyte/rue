"""Dispatch: every request gets a reply carrying its id, every refusal a reason."""

import unittest

from rue_hook import Execute, Hooks, Notify, Probe, Observation, Refusal
from rue_hook.serve import registration


def req(kind, op, **kw):
    return {"id": 42, "kind": kind, "op": op, **kw}


class HooksTest(unittest.TestCase):
    def test_an_op_the_protocol_does_not_have_is_refused_by_name(self):
        r = Hooks().answer(req("execute", "teleport"))
        self.assertEqual((r["id"], r["ok"]), (42, False))
        self.assertIn("execute.teleport", r["error"])
        self.assertFalse(Hooks().answer(req("weather", "report"))["ok"])

    def test_an_op_of_an_unserved_kind_is_refused_not_silent(self):
        r = Hooks().answer(req("journal", "append", entry={}))
        self.assertEqual((r["id"], r["ok"]), (42, False))
        self.assertIn("journal.append", r["error"])

    def test_a_handlers_refusal_and_a_handlers_fault_both_become_reasons(self):
        class N(Notify):
            def deliver(self, level, subject, body):
                if level == "warn":
                    raise Refusal("paging is off tonight")
                raise RuntimeError("socket closed")

        h = Hooks(notify=N())
        self.assertEqual(h.answer(req("notify", "deliver", level="warn"))["error"], "paging is off tonight")
        fault = h.answer(req("notify", "deliver", level="err"))
        self.assertFalse(fault["ok"])
        self.assertIn("RuntimeError: socket closed", fault["error"])

    def test_read_facts_no_such_file_is_an_answer_with_no_content(self):
        class E(Execute):
            def run(self, host, instance, body):
                return {"stdout": "", "outputs": {}}

            def read_fact(self, host, shape):
                return "" if shape.endswith("present") else None

        h = Hooks(execute=E())
        absent = h.answer(req("execute", "read_fact", shape="file:/absent"))
        self.assertTrue(absent["ok"])
        self.assertNotIn("content", absent)
        self.assertEqual(h.answer(req("execute", "read_fact", shape="file:/present"))["content"], "")

    def test_the_registration_names_only_the_kinds_supplied(self):
        class P(Probe):
            def observe(self, host, probe):
                return Observation.yes("")

        class N(Notify):
            def deliver(self, level, subject, body):
                pass

        reg = registration("x", Hooks(probe=P(), notify=N()))
        self.assertEqual(reg["kinds"], ["probe", "notify"])
        self.assertEqual(reg["protocol"], 1)


if __name__ == "__main__":
    unittest.main()
