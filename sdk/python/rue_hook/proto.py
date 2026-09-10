"""The hook protocol's wire, as data (docs/hook-protocol.md v1).

A transcription of `hook-proto/src/op.rs`, which is the protocol's one
source. It is checked against that source by `tools/lint-hook-ops.sh`, a
gate phase, so the two cannot drift: an op added in Rust and not here fails
the gate, and the reverse fails it too.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

HOOK_PROTOCOL = 1

#: The eight kinds a hook may register, in the order 7.5 lists them.
KINDS = (
    "journal",
    "inventory",
    "execute",
    "probe",
    "approval",
    "secrets",
    "notify",
    "scheduler",
)


@dataclass(frozen=True)
class Op:
    """One op: what its request carries and what its reply must."""

    kind: str
    op: str
    request: tuple[str, ...] = ()
    #: Fields an `ok: true` reply must carry; R0303 without them.
    required_reply: tuple[str, ...] = ()
    optional_reply: tuple[str, ...] = ()
    #: "to_hook" or "to_engine" for the four messages of 7.5 that may
    #: carry a secret; None for every other.
    secret: Optional[str] = None
    #: True only for `execute.clock`, which a hook may decline outright.
    optional: bool = False


OPS = (
    Op("journal", "append", request=("entry",)),
    Op("inventory", "list", required_reply=("hosts",)),
    Op(
        "execute",
        "run",
        request=("host", "instance", "body", "env", "secrets"),
        required_reply=("output",),
        optional_reply=("facts",),
        secret="to_hook",
    ),
    Op("execute", "read_fact", request=("host", "shape"), optional_reply=("content",)),
    Op("execute", "bootstrap_state", request=("host",), required_reply=("state",)),
    Op("execute", "clock", request=("host",), required_reply=("epoch_s",), optional=True),
    Op("execute", "instance_dir_create", request=("host", "instance")),
    Op("execute", "instance_dir_remove", request=("host", "instance")),
    Op("execute", "instance_dir_list", request=("host",), required_reply=("dirs",)),
    Op("execute", "put_file", request=("host", "instance", "rel", "content", "mode")),
    Op("execute", "replace_file", request=("host", "instance", "rel", "content")),
    Op("execute", "get_file", request=("host", "instance", "rel"), required_reply=("content",)),
    Op("execute", "remove_file", request=("host", "instance", "rel")),
    Op("execute", "host_lock", request=("host",)),
    Op("probe", "observe", request=("host", "probe"), required_reply=("fact",)),
    Op("approval", "authenticators", required_reply=("authenticators",)),
    Op(
        "approval",
        "challenge",
        request=("instance", "digest", "scope", "context"),
        required_reply=("challenge",),
    ),
    Op(
        "approval",
        "verify",
        request=("instance", "digest", "scope", "authenticator", "proof"),
        required_reply=("verified",),
        optional_reply=("reason",),
    ),
    Op("secrets", "resolve", request=("ref",), required_reply=("value",), secret="to_engine"),
    Op(
        "secrets",
        "deliver",
        request=("instance", "label", "value"),
        required_reply=("accepted",),
        optional_reply=("receipt",),
        secret="to_hook",
    ),
    Op("notify", "deliver", request=("level", "subject", "body")),
    Op("scheduler", "install", request=("host", "artifact", "deadline")),
    Op("scheduler", "arm", request=("host", "artifact", "deadline")),
    Op("scheduler", "rearm", request=("host", "artifact", "deadline")),
    Op("scheduler", "disarm", request=("host", "artifact", "deadline")),
    Op("scheduler", "present", request=("host", "artifact", "deadline"), required_reply=("present",)),
)

_BY_NAME = {(o.kind, o.op): o for o in OPS}


def find(kind: str, op: str) -> Optional[Op]:
    """The op by kind and name, or None for a pair with no row."""
    return _BY_NAME.get((kind, op))


#: What a secret's text reads as when a Resolved is formatted.
REDACTED = "<secret>"


@dataclass(frozen=True)
class Resolved:
    """A value after resolution: its text, and whether it is a secret.

    `repr` redacts a secret's text, so a body printed in a log line or a
    traceback does not spill one. The value is still reachable through the
    field; what is prevented is spilling it without meaning to.
    """

    text: str
    secret: bool = False

    def __repr__(self) -> str:
        shown = REDACTED if self.secret else self.text
        return f"Resolved(text={shown!r}, secret={self.secret!r})"

    @staticmethod
    def of(v: object) -> "Resolved":
        if isinstance(v, dict):
            return Resolved(str(v.get("text", "")), bool(v.get("secret", False)))
        return Resolved(str(v))


@dataclass
class RPrim:
    """One resolved primitive of an `execute.run` body.

    `name` is the primitive (`run`, `write`, `region_set`, ...) and
    `fields` its values, with every resolved value an `Resolved`.
    """

    name: str
    fields: dict = field(default_factory=dict)

    @staticmethod
    def parse(raw: dict) -> "RPrim":
        name = next(iter(raw), "")
        body = raw.get(name) or {}
        if not isinstance(body, dict):
            return RPrim(name, {"value": body})
        out = {}
        for k, v in body.items():
            if isinstance(v, dict) and "text" in v:
                out[k] = Resolved.of(v)
            elif k == "env" and isinstance(v, list):
                out[k] = [(pair[0], Resolved.of(pair[1])) for pair in v if len(pair) == 2]
            else:
                out[k] = v
        return RPrim(name, out)

    def carries_secret(self) -> bool:
        for v in self.fields.values():
            if isinstance(v, Resolved) and v.secret:
                return True
            if isinstance(v, list):
                for item in v:
                    if isinstance(item, tuple) and isinstance(item[1], Resolved) and item[1].secret:
                        return True
        return False
