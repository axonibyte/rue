"""The serve loop: one reply per request line, nothing for anything else,
a budget, and no line -- however malformed -- that ends the hook."""

import io
import json
import time
import unittest

from rue_hook import Hooks, Observation, Probe
from rue_hook.serve import _pump


class _Probe(Probe):
    def __init__(self, sleep=0.0, answer=None):
        self.sleep = sleep
        self.answer = answer

    def observe(self, host, probe):
        time.sleep(self.sleep)
        return self.answer if self.answer is not None else Observation.yes("ok")


def _req(i):
    return json.dumps({"id": i, "kind": "probe", "op": "observe", "host": "h", "probe": "up"})


def _strict(constant):
    # json.loads accepts NaN and Infinity by default; the engine does not.
    raise ValueError(f"{constant} is not JSON")


def _serve(lines, hooks, budget=None):
    out = io.StringIO()
    _pump(io.StringIO("\n".join(lines) + "\n"), out, hooks, budget)
    return [json.loads(l, parse_constant=_strict) for l in out.getvalue().splitlines() if l.strip()]


class ServeTest(unittest.TestCase):
    def test_only_requests_are_answered_and_each_once(self):
        replies = _serve(
            ["", "not json", '["an","array"]', '{"event":{"plan":"p"}}', '{"id":1}', _req(2), _req(3)],
            Hooks(probe=_Probe()),
        )
        self.assertEqual([r["id"] for r in replies], [2, 3])

    def test_a_pathologically_nested_line_does_not_end_the_hook(self):
        # json raises RecursionError, not ValueError, on deep nesting; the
        # loop caught only ValueError, so one line of brackets ended it.
        replies = _serve([_req(1), "[" * 200_000, _req(2)], Hooks(probe=_Probe()))
        self.assertEqual([r["id"] for r in replies], [1, 2])

    def test_a_handler_over_its_budget_answers_no_and_says_why(self):
        slow = _serve([_req(9)], Hooks(probe=_Probe(sleep=0.15)), budget=0.02)[0]
        self.assertEqual((slow["id"], slow["ok"]), (9, False))
        self.assertIn("budget", slow["error"])
        quick = _serve([_req(9)], Hooks(probe=_Probe()), budget=5.0)[0]
        self.assertTrue(quick["ok"])

    def test_a_reply_that_cannot_be_written_becomes_a_refusal_not_a_dead_hook(self):
        class Sets(Probe):
            def observe(self, host, probe):
                return Observation.yes({"a", "set"})  # not JSON

        replies = _serve([_req(5), _req(6)], Hooks(probe=Sets()))
        self.assertEqual([r["id"] for r in replies], [5, 6])
        self.assertFalse(replies[0]["ok"])

    def test_a_number_json_cannot_spell_is_refused_not_written(self):
        # json.dumps wrote a NaN as a bare NaN: a line that is not JSON,
        # which the engine cannot parse and reads as a silence.
        for bad in (float("nan"), float("inf"), -float("inf")):
            with self.subTest(bad=bad):
                replies = _serve([_req(7), _req(8)], Hooks(probe=_Probe(answer=Observation.yes(bad))))
                self.assertEqual([r["id"] for r in replies], [7, 8])
                self.assertFalse(replies[0]["ok"])
                self.assertIn("could not be written", replies[0]["error"])


if __name__ == "__main__":
    unittest.main()
