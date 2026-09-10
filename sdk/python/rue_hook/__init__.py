"""The Python embedding SDK for rue's hook protocol (docs/ROADMAP.md 7.11).

Standard library only, on purpose: a hook is often a simulator, a glue
script or a fixture living beside something else, and asking it to take a
dependency to answer a line of JSON would be a poor trade. It needs Python
3.11 and nothing further.

The shape matches `sdk/rust`: a class per kind, a `Hooks` that knows which
ones you filled in, and a serve loop that does the handshake.

    from rue_hook import Hooks, Probe, Observation, serve_stdio

    class Fence(Probe):
        def observe(self, host, probe):
            return Observation.yes("off") if fenced(host) else Observation.no("on")

    serve_stdio("fence", Hooks(probe=Fence()))

What the SDK is for, beyond the framing: a reply is built from the op's own
row in `OPS`, so a hook written on it cannot answer `ok: true` without a
field the op requires. That is R0303 at the engine, and R0303 refuses the
step -- the SDK's job is to make it unreachable by accident.

Secrets cross this boundary in exactly four messages (7.5). An
`execute.run` hands its handler the resolved body with the secrets intact;
`Resolved.__repr__` redacts one, so a body that reaches a log line or a
traceback does not carry the value with it.
"""

from .proto import OPS, KINDS, HOOK_PROTOCOL, Op, Resolved, RPrim
from .hooks import (
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
from .serve import serve_socket, serve_stdio

__all__ = [
    "OPS",
    "KINDS",
    "HOOK_PROTOCOL",
    "Op",
    "Resolved",
    "RPrim",
    "Approval",
    "Authenticator",
    "ChallengeRequest",
    "Delivery",
    "Execute",
    "Hooks",
    "Inventory",
    "Journal",
    "Notify",
    "Observation",
    "Presence",
    "Probe",
    "Refusal",
    "Scheduler",
    "Secrets",
    "Verdict",
    "serve_socket",
    "serve_stdio",
]
