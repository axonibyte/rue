"""A class per kind, and the dispatch from a request frame to a handler.

The registration frame is built from what you filled in, so a hook cannot
register for a kind it does not serve -- the failure that produces is a
plan binding to it and refusing at the first step, a long way from where
the mistake was made.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from .proto import KINDS, RPrim, find


class Refusal(Exception):
    """Why a hook will not answer: the text the engine journals and the
    operator reads. Raising it is a hook saying no, on the record -- not a
    fault."""

    @staticmethod
    def unserved(kind: str, op: str) -> "Refusal":
        return Refusal(f"this hook does not serve {kind}.{op}")


@dataclass
class Observation:
    """A probe's answer: its text and the three-valued reading a guard
    takes."""

    text: str = ""
    tri: Optional[str] = None

    @staticmethod
    def yes(text: str = "") -> "Observation":
        return Observation(text, "yes")

    @staticmethod
    def no(text: str = "") -> "Observation":
        return Observation(text, "no")

    @staticmethod
    def unknown(text: str = "") -> "Observation":
        return Observation(text, "unknown")

    def wire(self) -> dict:
        return {"text": self.text, "tri": self.tri}


@dataclass
class Authenticator:
    id: str
    human: bool


@dataclass
class ChallengeRequest:
    instance: str
    digest: str
    scope: object
    context: object


@dataclass
class VerifyRequest:
    instance: str
    digest: str
    scope: object
    authenticator: str
    proof: str


@dataclass
class Verdict:
    verified: bool
    reason: str = ""


@dataclass
class Delivery:
    accepted: bool
    receipt: str = ""


class Presence:
    PRESENT = True
    ABSENT = False
    #: Never guess: the engine reads `false` as "install it again", which
    #: is destructive when it is wrong.
    UNKNOWN = "unknown"


class Journal:
    def append(self, entry: dict) -> None:
        raise Refusal.unserved("journal", "append")


class Inventory:
    def list(self) -> list:
        raise Refusal.unserved("inventory", "list")


class Execute:
    """The instance-directory ops below `run` are required only of a hook
    that registers `filesystem` (7.7)."""

    def run(self, host: str, instance: str, body: list) -> dict:
        raise Refusal.unserved("execute", "run")

    def read_fact(self, host: str, shape: str) -> Optional[str]:
        raise Refusal.unserved("execute", "read_fact")

    def bootstrap_state(self, host: str) -> dict:
        raise Refusal.unserved("execute", "bootstrap_state")

    def clock(self, host: str) -> int:
        """The one op the engine reads a refusal of as "no skew probe is
        possible here" rather than as a fault."""
        raise Refusal.unserved("execute", "clock")

    def instance_dir_create(self, host: str, instance: str) -> None:
        raise Refusal.unserved("execute", "instance_dir_create")

    def instance_dir_remove(self, host: str, instance: str) -> None:
        raise Refusal.unserved("execute", "instance_dir_remove")

    def instance_dir_list(self, host: str) -> list:
        raise Refusal.unserved("execute", "instance_dir_list")

    def put_file(self, host: str, instance: str, rel: str, content: str, mode: int) -> None:
        raise Refusal.unserved("execute", "put_file")

    def replace_file(self, host: str, instance: str, rel: str, content: str) -> None:
        raise Refusal.unserved("execute", "replace_file")

    def get_file(self, host: str, instance: str, rel: str) -> str:
        raise Refusal.unserved("execute", "get_file")

    def remove_file(self, host: str, instance: str, rel: str) -> None:
        raise Refusal.unserved("execute", "remove_file")

    def host_lock(self, host: str) -> None:
        raise Refusal.unserved("execute", "host_lock")


class Probe:
    def observe(self, host: str, probe: str) -> Observation:
        raise Refusal.unserved("probe", "observe")


class Approval:
    def authenticators(self) -> list:
        raise Refusal.unserved("approval", "authenticators")

    def challenge(self, r: ChallengeRequest) -> str:
        raise Refusal.unserved("approval", "challenge")

    def verify(self, r: VerifyRequest) -> Verdict:
        raise Refusal.unserved("approval", "verify")


class Secrets:
    def resolve(self, reference: str) -> str:
        raise Refusal.unserved("secrets", "resolve")

    def deliver(self, instance: str, label: str, value: str) -> Delivery:
        raise Refusal.unserved("secrets", "deliver")


class Notify:
    def deliver(self, level: str, subject: str, body: str) -> None:
        raise Refusal.unserved("notify", "deliver")


class Scheduler:
    def install(self, host: str, artifact: str) -> None:
        raise Refusal.unserved("scheduler", "install")

    def arm(self, host: str, artifact: str, deadline: Optional[int]) -> None:
        raise Refusal.unserved("scheduler", "arm")

    def rearm(self, host: str, artifact: str, deadline: Optional[int]) -> None:
        raise Refusal.unserved("scheduler", "rearm")

    def disarm(self, host: str, artifact: str) -> None:
        raise Refusal.unserved("scheduler", "disarm")

    def present(self, host: str, artifact: str):
        raise Refusal.unserved("scheduler", "present")


@dataclass
class Hooks:
    """What this hook serves. Fill in the kinds you implement."""

    journal: Optional[Journal] = None
    inventory: Optional[Inventory] = None
    execute: Optional[Execute] = None
    probe: Optional[Probe] = None
    approval: Optional[Approval] = None
    secrets: Optional[Secrets] = None
    notify: Optional[Notify] = None
    scheduler: Optional[Scheduler] = None
    #: This hook serves the instance-directory ops (7.7).
    filesystem: bool = False
    stdin_preamble: bool = False

    def kinds(self) -> list:
        return [k for k in KINDS if getattr(self, k) is not None]

    def answer(self, request: dict) -> dict:
        """Answer one request frame.

        The reply always carries the request's id, including for a request
        this hook has no handler for: an unanswered request is Silent,
        which tells the operator that a step did not happen and nothing
        about why.
        """
        rid = request.get("id")
        kind = request.get("kind") or ""
        op_name = request.get("op") or ""
        row = find(kind, op_name)
        if row is None:
            return {
                "id": rid,
                "ok": False,
                "error": f"{kind}.{op_name} is not an op of this protocol",
            }
        try:
            fields = self._dispatch(row, request)
        except Refusal as why:
            return {"id": rid, "ok": False, "error": str(why)}
        except Exception as e:  # a handler that raised is a refusal, not a silence
            return {"id": rid, "ok": False, "error": f"{type(e).__name__}: {e}"}
        reply = {"id": rid, "ok": True}
        reply.update(fields)
        missing = [f for f in row.required_reply if f not in reply]
        if missing:
            # Replies are built from the op's row, so this is a bug in the SDK
            # and not an R0303 for the far end to puzzle over. Refused by
            # name, as every other SDK does: an assert here vanished under
            # `python -O`, and when it fired it ended the serve loop.
            return {
                "id": rid,
                "ok": False,
                "error": f"the SDK built a {kind}.{op_name} reply without {', '.join(missing)}",
            }
        return reply

    def _handler(self, kind: str, row):
        h = getattr(self, kind)
        if h is None:
            raise Refusal.unserved(row.kind, row.op)
        return h

    def _dispatch(self, row, r: dict) -> dict:
        kind, op = row.kind, row.op
        h = self._handler(kind, row)
        s = lambda k: r.get(k) or ""  # noqa: E731 -- a local reader, not a function
        if kind == "journal":
            entry = r.get("entry")
            if entry is None:
                raise Refusal("journal.append without an entry")
            h.append(entry)
            return {}
        if kind == "inventory":
            return {"hosts": h.list()}
        if kind == "probe":
            return {"fact": h.observe(s("host"), s("probe")).wire()}
        if kind == "notify":
            h.deliver(s("level"), s("subject"), s("body"))
            return {}
        if kind == "approval":
            if op == "authenticators":
                return {
                    "authenticators": [
                        {"id": a.id, "human": a.human} for a in h.authenticators()
                    ]
                }
            if op == "challenge":
                return {
                    "challenge": h.challenge(
                        ChallengeRequest(s("instance"), s("digest"), r.get("scope"), r.get("context"))
                    )
                }
            v = h.verify(
                VerifyRequest(
                    s("instance"), s("digest"), r.get("scope"), s("authenticator"), s("proof")
                )
            )
            return {"verified": v.verified, "reason": v.reason}
        if kind == "secrets":
            if op == "resolve":
                return {"value": h.resolve(s("ref"))}
            d = h.deliver(s("instance"), s("label"), s("value"))
            return {"accepted": d.accepted, "receipt": d.receipt}
        if kind == "scheduler":
            host, artifact = s("host"), s("artifact")
            deadline = r.get("deadline")
            if op == "install":
                h.install(host, artifact)
            elif op == "arm":
                h.arm(host, artifact, deadline)
            elif op == "rearm":
                h.rearm(host, artifact, deadline)
            elif op == "disarm":
                h.disarm(host, artifact)
            else:
                return {"present": h.present(host, artifact)}
            return {}
        # execute
        if op == "run":
            body = [RPrim.parse(p) for p in (r.get("body") or [])]
            return {"output": h.run(s("host"), s("instance"), body)}
        if op == "read_fact":
            content = h.read_fact(s("host"), s("shape"))
            # None is "no such file", which is an answer. An empty string
            # would be a file that exists and is empty.
            return {} if content is None else {"content": content}
        if op == "bootstrap_state":
            return {"state": h.bootstrap_state(s("host"))}
        if op == "clock":
            return {"epoch_s": h.clock(s("host"))}
        if op == "instance_dir_create":
            h.instance_dir_create(s("host"), s("instance"))
            return {}
        if op == "instance_dir_remove":
            h.instance_dir_remove(s("host"), s("instance"))
            return {}
        if op == "instance_dir_list":
            return {"dirs": h.instance_dir_list(s("host"))}
        if op == "put_file":
            h.put_file(s("host"), s("instance"), s("rel"), s("content"), int(r.get("mode") or 0))
            return {}
        if op == "replace_file":
            h.replace_file(s("host"), s("instance"), s("rel"), s("content"))
            return {}
        if op == "get_file":
            return {"content": h.get_file(s("host"), s("instance"), s("rel"))}
        if op == "remove_file":
            h.remove_file(s("host"), s("instance"), s("rel"))
            return {}
        h.host_lock(s("host"))
        return {}
