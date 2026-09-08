#!/usr/bin/env python3
"""T1's hooks, as a daemon-spawned child (docs/hook-protocol.md).

One file, three hooks, chosen by the first argument:

  authority   the approval binding: two human authenticators, a challenge
              over the digest rue supplies, and a verdict on a proof
  escrow      a secrets acceptor: it takes the credential and keeps a
              receipt, and never writes the value anywhere
  bmc_api     the management controller: an `execute` and `probe` hook for
              a host with no filesystem, standing in for the appliance T1
              enables an account on

Standard library only, and no state outside the directory given by
RUE_T1_STATE (a temporary directory the harness makes). What it simulates
is a real appliance's API: an account that is enabled, disabled, and
readable as a fact.

The protocol: the first line this writes is its `register` frame, the
first line it reads is the acknowledgement, and after that it reads one
request per line and writes one reply per line.
"""

import json
import os
import sys

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


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def register(name, kinds, filesystem=False):
    send(
        {
            "register": {
                "name": name,
                "kinds": kinds,
                "protocol": 1,
                "filesystem": filesystem,
                "stdin_preamble": False,
            }
        }
    )
    # The acknowledgement; anything else is fatal, and saying so on stderr
    # is what the daemon prints.
    line = sys.stdin.readline()
    if not line:
        sys.exit("no acknowledgement from rued")
    ack = json.loads(line)
    if not ack.get("register", {}).get("ok"):
        sys.exit("registration refused: %s" % line.strip())


# --- authority ---------------------------------------------------------

AUTHENTICATORS = [
    {"id": "oncall", "human": True},
    {"id": "second", "human": True},
]


def authority(req):
    op = req.get("op")
    if op == "authenticators":
        return {"authenticators": AUTHENTICATORS}
    if op == "challenge":
        # Rue supplies the digest; what a human is shown is the binding's
        # business, and this one shows the digest's first bytes.
        digest = req.get("digest", "")
        scope = json.dumps(req.get("scope"))
        return {"challenge": "approve %s scope %s [%s]" % (req.get("instance"), scope, digest[:16])}
    if op == "verify":
        proof = req.get("proof", "")
        who = req.get("authenticator", "")
        if who not in [a["id"] for a in AUTHENTICATORS]:
            return {"verified": False, "reason": "%s is not an authenticator here" % who}
        # The proof this stub accepts is the digest itself, which is what
        # a real one would check a signature over. An empty or wrong proof
        # is refused, so a test can prove a refusal too.
        if proof and req.get("digest", "").startswith(proof[:8]):
            return {"verified": True, "reason": ""}
        return {"verified": False, "reason": "the token does not match the digest"}
    return None


# --- escrow ------------------------------------------------------------


def escrow(req):
    if req.get("op") == "deliver":
        # The value is kept in memory only long enough to answer; what is
        # written is the label and a receipt, never the credential.
        label = req.get("label", "")
        receipts = read_state("escrow-receipts")
        write_state("escrow-receipts", receipts + label + "\n")
        return {"accepted": True, "receipt": "escrow-%d" % (len(receipts.splitlines()) + 1)}
    return None


# --- bmc_api -----------------------------------------------------------

PASSWORD = "t1-breakglass-password"


def bmc_run(body):
    """The primitives the engine sends: this appliance serves `hook` only."""
    outputs = {}
    for prim in body:
        if "hook" in prim:
            name = prim["hook"].get("name", "")
            if name == "bmc_enable":
                write_state("bmc-account", "enabled\n")
                outputs["bmc_password"] = PASSWORD
            elif name == "bmc_disable":
                write_state("bmc-account", "")
            else:
                return None
        else:
            # A file primitive on a host with no filesystem: refused, which
            # is what R0408 exists to prevent reaching in the first place.
            return None
    return {"output": {"stdout": "", "outputs": outputs}, "facts": {}}


def bmc(req):
    kind, op = req.get("kind"), req.get("op")
    if kind == "execute" and op == "run":
        return bmc_run(req.get("body", []))
    if kind == "execute" and op == "read_fact":
        # The account fact, as the appliance reports it.
        return {"content": read_state("bmc-account")}
    if kind == "execute" and op == "clock":
        import time

        return {"epoch_s": int(time.time())}
    if kind == "execute" and op == "bootstrap_state":
        # No filesystem: nothing to bootstrap, and the engine never asks
        # this host for an instance directory.
        return {
            "state": {
                "rue_root": False,
                "group": False,
                "instances_dir": False,
                "lock": False,
                "modes_ok": False,
            }
        }
    if kind == "probe" and op == "observe":
        enabled = read_state("bmc-account").strip() == "enabled"
        return {"fact": {"text": read_state("bmc-account").strip(), "tri": "yes" if enabled else "no"}}
    return None


HOOKS = {
    "authority": (["approval"], authority),
    "escrow": (["secrets"], escrow),
    "bmc_api": (["execute", "probe"], bmc),
}


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in HOOKS:
        sys.exit("usage: hooks.py <%s>" % "|".join(sorted(HOOKS)))
    name = sys.argv[1]
    kinds, serve = HOOKS[name]
    register(name, kinds)
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        req = json.loads(line)
        rid = req.get("id")
        try:
            answer = serve(req)
        except Exception as e:  # a hook that breaks says so, and rue refuses
            send({"id": rid, "ok": False, "error": "%s: %s" % (type(e).__name__, e)})
            continue
        if answer is None:
            send({"id": rid, "ok": False, "error": "%s does not serve %s.%s" % (name, req.get("kind"), req.get("op"))})
        else:
            reply = {"id": rid, "ok": True}
            reply.update(answer)
            send(reply)


if __name__ == "__main__":
    main()
