#!/usr/bin/env python3
"""T1's hooks, as daemon-spawned children, written on the Python SDK.

One file, three hooks, chosen by the first argument:

  authority   the approval binding: two human authenticators, a challenge
              over the digest rue supplies, and a verdict on a proof
  escrow      a secrets acceptor: it takes the credential and keeps a
              receipt, and never writes the value anywhere
  bmc_api     the management controller: an `execute` and `probe` hook for
              a host with no filesystem, standing in for the appliance T1
              enables an account on

No state outside the directory given by RUE_T1_STATE (a temporary
directory the harness makes). What it simulates is a real appliance's API:
an account that is enabled, disabled, and readable as a fact.

This is the SDK's first real user, and it is here rather than in a test
because the framing, the handshake and the reply-building are exactly what
a tenant should not have to write again. What is left in this file is only
what is T1's: which authenticators exist, what the appliance does, and
where the receipts go.
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
    Delivery,
    Execute,
    Hooks,
    Observation,
    Probe,
    Refusal,
    Secrets,
    Verdict,
    serve_stdio,
)

STATE = os.environ.get("RUE_T1_STATE", "/tmp/rue-t1-state")


def state_path(name):
    os.makedirs(STATE, exist_ok=True)
    return os.path.join(STATE, name)


def read_state(name, default=""):
    try:
        with open(state_path(name), "r", encoding="utf-8") as f:
            return f.read()
    except FileNotFoundError:
        return default


def write_state(name, text):
    with open(state_path(name), "w", encoding="utf-8") as f:
        f.write(text)


# --- authority ---------------------------------------------------------

AUTHENTICATORS = [Authenticator("oncall", True), Authenticator("second", True)]


class Authority(Approval):
    def authenticators(self):
        return AUTHENTICATORS

    def challenge(self, r: ChallengeRequest):
        # Rue supplies the digest; what a human is shown is the binding's
        # business, and this one shows the digest's first bytes.
        return f"approve {r.instance} scope {r.scope} [{r.digest[:16]}]"

    def verify(self, r):
        if r.authenticator not in [a.id for a in AUTHENTICATORS]:
            return Verdict(False, f"{r.authenticator} is not an authenticator here")
        # The proof this stub accepts is the digest itself, which is what a
        # real one would check a signature over. An empty or wrong proof is
        # refused, so a test can prove a refusal too.
        if r.proof and r.digest.startswith(r.proof[:8]):
            return Verdict(True, "")
        return Verdict(False, "the token does not match the digest")


# --- escrow ------------------------------------------------------------


class Escrow(Secrets):
    def deliver(self, instance, label, value):
        # The value is held only long enough to answer; what is written is
        # the label and a receipt, never the credential.
        receipts = read_state("escrow-receipts")
        write_state("escrow-receipts", receipts + label + "\n")
        return Delivery(True, "escrow-%d" % (len(receipts.splitlines()) + 1))


# --- bmc_api -----------------------------------------------------------

PASSWORD = "t1-breakglass-password"


class Bmc(Execute, Probe):
    def run(self, host, instance, body):
        """The primitives the engine sends: this appliance serves `hook`."""
        outputs = {}
        for prim in body:
            if prim.name != "hook":
                # A file primitive on a host with no filesystem, which is
                # what R0408 exists to stop reaching here at all.
                raise Refusal(f"the appliance serves no {prim.name} primitive")
            name = prim.fields.get("name", "")
            if name == "bmc_enable":
                write_state("bmc-account", "enabled\n")
                outputs["bmc_password"] = PASSWORD
            elif name == "bmc_disable":
                write_state("bmc-account", "")
            else:
                raise Refusal(f"the appliance has no operation named {name}")
        return {"stdout": "", "outputs": outputs}

    def read_fact(self, host, shape):
        # The account fact, as the appliance reports it.
        return read_state("bmc-account")

    def clock(self, host):
        return int(time.time())

    def bootstrap_state(self, host):
        # No filesystem: nothing to bootstrap, and the engine never asks
        # this host for an instance directory.
        return {
            "rue_root": False,
            "group": False,
            "instances_dir": False,
            "lock": False,
            "modes_ok": False,
        }

    def observe(self, host, probe):
        account = read_state("bmc-account").strip()
        return Observation.yes(account) if account == "enabled" else Observation.no(account)


HOOKS = {
    "authority": lambda: Hooks(approval=Authority()),
    "escrow": lambda: Hooks(secrets=Escrow()),
    "bmc_api": lambda: (lambda b: Hooks(execute=b, probe=b))(Bmc()),
}


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in HOOKS:
        sys.exit("usage: hooks.py <%s>" % "|".join(sorted(HOOKS)))
    name = sys.argv[1]
    serve_stdio(name, HOOKS[name]())


if __name__ == "__main__":
    main()
