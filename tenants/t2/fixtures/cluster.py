#!/usr/bin/env python3
"""T2's pseudo-cluster, as daemon-spawned children on the Python SDK.

One file, two hooks, chosen by the first argument:

  cluster     the driver the controller is reached through (`execute via:
              hook(:cluster, transport: :controller)`). It performs the
              fence, the platform's slave mode and the placement record,
              appends the succession log, and answers the promote ladder's
              probes by name.
  authority   the approval binding: the manual path's human
              acknowledgements, verified against the digest rue supplies.

node-b is the guest itself, reached over ssh, and the jails its steps start
are real. node-a, the corpse, is never reached at all: everything done to
it is done THROUGH this driver, exactly as tenants/t2/plan.rue reaches it.

All state lives in RUE_T2_STATE, a directory the harness makes and reads.
The harness steers the cluster by writing four knobs there:

  fence        what the fence driver reports: `off` (verified), `unknown`,
               or `on`. A knell's guard is three-valued, and each value is
               a path through 8.2.
  written      bytes written to the corpse's datasets since the split: the
               static preflight's reading, frozen at request and measured
               again at the acknowledgement.
  heir         `running` once the heir is up on node-c, which is what the
               reap pass's handoff probe asks.
  placement_refuses
               an entry the placement service refuses to record: a refusal
               after the fence, where 8.2's auto promote holds. Only that
               entry -- its compensation is recorded, as a service that
               rejects one bad write still accepts the next.
"""

import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "sdk" / "python"))

from rue_hook import (  # noqa: E402
    Approval,
    Authenticator,
    ChallengeRequest,
    Execute,
    Hooks,
    Observation,
    Probe,
    Refusal,
    Verdict,
    serve_stdio,
)

STATE = os.environ.get("RUE_T2_STATE", "/tmp/rue-t2-state")


def path(name):
    os.makedirs(STATE, exist_ok=True)
    return os.path.join(STATE, name)


def read(name, default=""):
    try:
        with open(path(name), "r", encoding="utf-8") as f:
            return f.read().strip()
    except FileNotFoundError:
        return default


def write(name, text):
    with open(path(name), "w", encoding="utf-8") as f:
        f.write(text + "\n")


def record(line):
    """Everything the driver did, in order, for the harness to read."""
    with open(path("actions"), "a", encoding="utf-8") as f:
        f.write(line + "\n")


def arg(prim, key):
    for pair in prim.fields.get("args") or []:
        if len(pair) == 2 and pair[0] == key:
            v = pair[1]
            return (v.get("text", "") if isinstance(v, dict) else str(v)).lstrip(":")
    return ""


# --- cluster -----------------------------------------------------------


class Cluster(Execute, Probe):
    def run(self, host, instance, body):
        for prim in body:
            if prim.name == "hook":
                self.action(prim)
            elif prim.name == "append":
                # The succession log is append-only: a line is added, never
                # rewritten, and the undo is a compensating line (8.2).
                shape = prim.fields.get("shape", "")
                if not shape.startswith("file:"):
                    raise Refusal(f"the driver appends only to files, not {shape}")
                line = prim.fields.get("line")
                with open(shape[len("file:"):], "a", encoding="utf-8") as f:
                    f.write((line.text if line else "") + "\n")
            else:
                # A primitive this driver does not implement is a thing rue
                # asked for and did not get; never silently.
                raise Refusal(f"the cluster driver has no {prim.name} primitive")
        return {"stdout": "", "outputs": {}}

    def action(self, prim):
        name = prim.fields.get("name", "")
        if name == "fence":
            node = arg(prim, "node")
            write(f"fenced-{node}", "fenced")
            record(f"fence {node}")
        elif name == "platform":
            node, mode = arg(prim, "node"), arg(prim, "set")
            write(f"platform-{node}", mode)
            record(f"platform {node} {mode}")
        elif name == "placement":
            entry = arg(prim, "set")
            if entry and entry == read("placement_refuses"):
                raise Refusal(f"the placement service refuses {entry!r}")
            with open(path("placement"), "a", encoding="utf-8") as f:
                f.write(entry + "\n")
            record(f"placement {entry}")
        else:
            raise Refusal(f"the cluster driver has no action named {name}")

    def read_fact(self, host, shape):
        if shape.startswith("platform:mode:"):
            return read("platform-" + shape.split(":", 2)[2], "master")
        if shape == "record:placement":
            return read("placement")
        if shape.startswith("file:"):
            try:
                with open(shape[len("file:"):], "r", encoding="utf-8") as f:
                    return f.read()
            except FileNotFoundError:
                return None
        return None

    def clock(self, host):
        return int(time.time())

    def bootstrap_state(self, host):
        # The controller keeps no instance directory: every T2 step that
        # runs here reverts only while the engine lives, or not at all.
        return {"rue_root": False, "group": False, "instances_dir": False,
                "lock": False, "modes_ok": False}

    def observe(self, host, probe):
        if probe == "peer_dead":
            return Observation.yes("node-a has missed every heartbeat since the split")
        if probe == "probes_agree":
            return Observation.yes("three independent probes agree node-a is gone")
        if probe == "fence_verified_off":
            state = read("fence", "off")
            if state == "off":
                return Observation.yes("the fence driver verified node-a off")
            if state == "on":
                return Observation.no("the fence driver reports node-a still powered")
            return Observation.unknown("the fence driver cannot tell")
        if probe == "fence_verdict":
            return Observation.yes("node-a: power off, confirmed by the fence driver")
        if probe == "written_bytes_since_split":
            return Observation.yes(read("written", "0"))
        if probe == "heir_running_on_c":
            if read("heir") == "running":
                return Observation.yes("the heir answers on node-c")
            return Observation.unknown("no word from node-c yet")
        raise Refusal(f"the cluster driver answers no probe named {probe}")


# --- authority ---------------------------------------------------------

AUTHENTICATORS = [
    Authenticator("operator", True),
    Authenticator("second_operator", True),
    Authenticator("fence_driver", False),
]


class Authority(Approval):
    def authenticators(self):
        return AUTHENTICATORS

    def challenge(self, r: ChallengeRequest):
        return f"acknowledge {r.instance} scope {r.scope} [{r.digest[:16]}]"

    def verify(self, r):
        if r.authenticator not in [a.id for a in AUTHENTICATORS]:
            return Verdict(False, f"{r.authenticator} is not an authenticator here")
        # The token this stub accepts is the digest's first bytes, which is
        # what a real one would check a signature over; a wrong or empty one
        # is refused, so a test can prove a refusal too.
        if r.proof and r.digest.startswith(r.proof[:8]):
            return Verdict(True, "")
        return Verdict(False, "the token does not match the digest")


HOOKS = {
    "cluster": lambda: (lambda c: Hooks(execute=c, probe=c))(Cluster()),
    "authority": lambda: Hooks(approval=Authority()),
}


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in HOOKS:
        sys.exit("usage: cluster.py <%s>" % "|".join(sorted(HOOKS)))
    name = sys.argv[1]
    serve_stdio(name, HOOKS[name]())


if __name__ == "__main__":
    main()
