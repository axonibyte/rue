"""The two ways `rued` reaches a hook (docs/hook-protocol.md).

Both do the same three things: send the registration frame, read the
acknowledgement, then answer one request per line until the far end
closes. Over the socket there is a `hello` first, and events from any
subscription arrive interleaved with requests -- they are skipped here,
because a hook that is not also an operator has nothing to do with them.
"""

from __future__ import annotations

import json
import socket
import sys
import time
from typing import Optional

from .hooks import Hooks
from .proto import HOOK_PROTOCOL, Resolved


def registration(name: str, hooks: Hooks) -> dict:
    return {
        "name": name,
        "kinds": hooks.kinds(),
        "protocol": HOOK_PROTOCOL,
        "filesystem": hooks.filesystem,
        "stdin_preamble": hooks.stdin_preamble,
    }


def _encode(v: object) -> str:
    # A Resolved a handler put in its reply is written as it formats: a
    # secret as its redaction, never its text. Anything else json cannot
    # write is the handler's mistake, refused by name in _pump.
    if isinstance(v, Resolved):
        return str(v)
    raise TypeError(f"Object of type {type(v).__name__} is not JSON serializable")


def _write(out, frame: dict) -> None:
    # Serialized before anything is written, so a frame that cannot be
    # written leaves no half line behind. allow_nan=False because json
    # otherwise spells a NaN or an infinity as a bare NaN or Infinity, which
    # is not JSON: the engine cannot parse the line, and a reply it cannot
    # parse reads as a silence. Flushed every time: an unflushed reply is a
    # silence, and silence is a refusal of the step with nothing to tell the
    # operator.
    text = json.dumps(frame, default=_encode, allow_nan=False)
    out.write(text + "\n")
    out.flush()


def _pump(reader, writer, hooks: Hooks, budget: Optional[float]) -> None:
    for line in reader:
        line = line.strip()
        if not line:
            continue
        try:
            frame = json.loads(line)
        except (ValueError, RecursionError):
            # RecursionError is what json raises on deep nesting, and it is
            # not a ValueError: one line of brackets ended the hook.
            continue
        if not isinstance(frame, dict) or "event" in frame or "kind" not in frame:
            continue
        started = time.monotonic()
        reply = hooks.answer(frame)
        took = time.monotonic() - started
        if budget is not None and took > budget and reply.get("ok") is True:
            # The engine's deadline is not on the wire, so an SDK cannot
            # see it. What it can do is keep its own slowness from
            # arriving as a silence, which says nothing about why.
            reply = {
                "id": frame.get("id"),
                "ok": False,
                "error": (
                    f"the handler took {took * 1000:.0f}ms, over its {budget * 1000:.0f}ms "
                    "budget; answering late is worse than answering no"
                ),
            }
        try:
            _write(writer, reply)
        except (TypeError, ValueError) as e:
            # A reply json cannot write -- a set, a NaN a handler put in it --
            # is refused by name rather than ending the loop.
            _write(writer, {"id": frame.get("id"), "ok": False,
                            "error": f"the reply could not be written: {e}"})


def serve_stdio(name: str, hooks: Hooks, budget: Optional[float] = None) -> None:
    """Serve as a child the daemon spawned (`rued run --spawn`).

    The registration frame is the first line of stdout, before anything
    else, so keep your own logging on stderr.
    """
    _write(sys.stdout, {"register": registration(name, hooks)})
    ack = sys.stdin.readline()
    if not ack or json.loads(ack).get("register", {}).get("ok") is not True:
        raise SystemExit(f"registration was not acknowledged: {ack.strip()!r}")
    _pump(sys.stdin, sys.stdout, hooks, budget)


def serve_socket(
    path: str,
    name: str,
    hooks: Hooks,
    identity: Optional[str] = None,
    budget: Optional[float] = None,
) -> None:
    """Serve over the control socket: `hello`, then `register`, then the
    same loop."""
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(path)
    with s.makefile("r") as reader, s.makefile("w") as writer:
        hello = {"id": 0, "verb": "hello", "proto": 1}
        if identity:
            hello["identity"] = identity
        _write(writer, {"hello": {"proto": 1, "identity": identity}})
        reply = json.loads(reader.readline() or "{}")
        if reply.get("hello", {}).get("ok") is not True and reply.get("ok") is not True:
            raise SystemExit(f"the daemon refused the hello: {reply}")
        _write(writer, {"register": registration(name, hooks)})
        ack = json.loads(reader.readline() or "{}")
        if ack.get("register", {}).get("ok") is not True:
            raise SystemExit(f"registration was refused: {ack}")
        _pump(reader, writer, hooks, budget)
