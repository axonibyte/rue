#!/usr/bin/env python3
"""The reference conformance hook for the Python SDK.

docs/sdk-conformance.md's fixed world, served over stdio: what
`rue sdk-conform` is pointed at to judge this SDK, and the worked example
an embedder copies.

The exception is the four provocations of the `probe` kind, which
deliberately violate the protocol. Those cannot go through `Hooks.answer`,
because it builds replies from the op's own row and an `ok: true` without
a required field is not expressible -- that is the guarantee the SDK
exists for. So this file drops to the wire for exactly those, and for
nothing else.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rue_hook import (  # noqa: E402
    Approval,
    Authenticator,
    ChallengeRequest,
    Delivery,
    Execute,
    Hooks,
    Inventory,
    Journal,
    Notify,
    Observation,
    Presence,
    Probe,
    Refusal,
    Scheduler,
    Secrets,
    Verdict,
)
from rue_hook.proto import Resolved  # noqa: E402
from rue_hook.serve import registration  # noqa: E402


class World(Journal, Inventory, Execute, Probe, Approval, Secrets, Notify, Scheduler):
    """Every kind over the contract's fixed world, which is deliberately
    stateless: what is under test is the protocol, not storage."""

    # journal
    def append(self, entry):
        return None

    # inventory
    def list(self):
        return [
            {
                "name": "conform-full",
                "address": "198.51.100.7",
                "os": "freebsd",
                "roles": ["a", "b"],
                "reach": ["hook"],
                "filesystem": True,
                "stdin_preamble": False,
                "scheduler": "cron",
                "rue_root": "/var/db/rue",
                "artifact": "python",
                "facts": {"site": "west"},
            },
            # Only what Appendix C requires; the rest takes its default.
            {"name": "conform-bare", "os": "linux"},
        ]

    # execute
    def run(self, host, instance, body):
        prim = body[0]
        cmd = prim.fields.get("cmd")
        env = dict(prim.fields.get("env") or [])
        pw = env.get("PW")
        if not isinstance(pw, Resolved):
            raise Refusal("the contract's run carries PW")
        return {
            "stdout": f"ran {len(body)} primitive\n",
            # `execute.run` carries a secret in both directions (7.5). An
            # SDK that scrubbed it on the way in, or could not reach it
            # without formatting it into a string, fails here.
            "outputs": {"echo": cmd.text, "secret": pw.text},
        }

    def read_fact(self, host, shape):
        return "present\n" if shape == "file:/conformance/present" else None

    def bootstrap_state(self, host):
        return {
            "rue_root": True,
            "group": True,
            "instances_dir": True,
            "lock": True,
            "modes_ok": True,
        }

    def clock(self, host):
        return 1700000000

    def instance_dir_create(self, host, instance):
        return None

    def instance_dir_remove(self, host, instance):
        return None

    def instance_dir_list(self, host):
        return [{"instance": "conform-1", "armed": True, "fired": False, "modes_ok": True}]

    def put_file(self, host, instance, rel, content, mode):
        return None

    def replace_file(self, host, instance, rel, content):
        return None

    def get_file(self, host, instance, rel):
        return "1700000000\n"

    def remove_file(self, host, instance, rel):
        return None

    def host_lock(self, host):
        return None

    # probe
    def observe(self, host, probe):
        if probe == "conform-yes":
            return Observation.yes("yes")
        if probe == "conform-no":
            return Observation.no("no")
        if probe == "conform-unknown":
            return Observation.unknown("")
        if probe == "conform-refuse":
            raise Refusal("refused as the conformance contract asks, with a reason to read")
        raise Refusal(f"no probe named {probe}")

    # approval
    def authenticators(self):
        return [Authenticator("conform-human", True), Authenticator("conform-machine", False)]

    def challenge(self, r: ChallengeRequest):
        return f"approve {r.digest} on {r.instance} ({scope_text(r.scope)})"

    def verify(self, r):
        # Bound to the digest *and* the scope (5.11). Built from what the
        # request carries, never from anything remembered, which is what
        # makes a replay fail.
        want = f"{r.digest}/{scope_text(r.scope)}"
        if r.proof == want:
            return Verdict(True, "")
        return Verdict(False, "the proof was made for another request or another scope")

    # secrets
    def resolve(self, reference):
        return "conformance-resolved-secret"

    def deliver(self, instance, label, value):
        # Declining is an answer, not a refusal: the engine offers the
        # secret to the next acceptor.
        return Delivery(label != "unwanted", f"receipt-{label}")

    # notify
    def deliver_notification(self, level, subject, body):
        return None

    # scheduler
    def install(self, host, artifact):
        return None

    def arm(self, host, artifact, deadline):
        return None

    def rearm(self, host, artifact, deadline):
        return None

    def disarm(self, host, artifact):
        return None

    def present(self, host, artifact):
        if artifact == "conform-present.sh":
            return Presence.PRESENT
        if artifact == "conform-absent.sh":
            return Presence.ABSENT
        # Never guess: the engine reads `false` as "install it again".
        return Presence.UNKNOWN


def scope_text(scope) -> str:
    """`plan`, `step/<n>`, `ack/<n>` -- the scope as the contract spells
    it, in a form every language builds the same way."""
    if scope == "plan":
        return "plan"
    if isinstance(scope, dict):
        if "step" in scope:
            return f"step/{scope['step']}"
        if "ack" in scope:
            return f"ack/{scope['ack']}"
    return "unknown"


class Notifier(Notify):
    def deliver(self, level, subject, body):
        return None


PROVOCATIONS = ("conform-missing-field", "conform-no-ok", "conform-silent")


def main() -> int:
    name = sys.argv[1] if len(sys.argv) > 1 else "conform"
    world = World()
    hooks = Hooks(
        journal=world,
        inventory=world,
        execute=world,
        probe=world,
        approval=world,
        secrets=world,
        # `deliver` is Secrets' method name too, so notify gets its own
        # object rather than a name collision nobody would enjoy debugging.
        notify=Notifier(),
        scheduler=world,
        filesystem=True,
        stdin_preamble=True,
    )
    out = sys.stdout
    out.write(json.dumps({"register": registration(name, hooks)}) + "\n")
    out.flush()
    ack = sys.stdin.readline()
    if not ack or json.loads(ack).get("register", {}).get("ok") is not True:
        print(f"conform-hook: registration not acknowledged: {ack!r}", file=sys.stderr)
        return 1

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        frame = json.loads(line)
        if "kind" not in frame:
            continue
        rid = frame.get("id")
        probe = frame.get("probe")
        if frame.get("kind") == "probe" and probe in PROVOCATIONS:
            # Deliberately malformed, and deliberately not through the SDK.
            if probe == "conform-missing-field":
                out.write(json.dumps({"id": rid, "ok": True}) + "\n")
            elif probe == "conform-no-ok":
                out.write(json.dumps({"id": rid}) + "\n")
            # conform-silent: say nothing at all.
            out.flush()
            continue
        out.write(json.dumps(hooks.answer(frame)) + "\n")
        out.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
