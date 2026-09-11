# The hook protocol, version 1

How `rued` talks to a hook (docs/ROADMAP.md 7.5): newline-delimited JSON,
on the control socket after `hello` and `register`
(docs/control-protocol.md), or on the stdio of a child the daemon spawned.
The engine sends requests; the hook answers each by id. A hook serves the
kinds it registered.

**Version 1 is frozen.** `docs/hook-protocol-v1.json` is this protocol as
data -- every kind, every op, the fields each request sends and each reply
must and may carry, and which four messages may carry a secret -- generated
from `rue-hook-proto`'s tables, the same ones the engine, every SDK's guard
and the conformance runner read. Its digest is pinned in
`tools/lint-hook-proto-frozen.sh`, so the document cannot be regenerated in
place: a change to an op, a field or a kind is a new protocol version, with
`HOOK_PROTOCOL` bumped, a `-v2.json` beside this one, and v1's bytes left
where they are for anything still speaking it. A hook written against this
page today keeps working until something says, in a new document, that it
will not.

```
engine → hook: {"id": 7, "kind": "<kind>", "op": "<op>", ...}
hook → engine: {"id": 7, "ok": true, ...}
hook → engine: {"id": 7, "ok": false, "error": "why"}
```

Every request has a deadline (`rued run --hook-deadline`, 30 s by default).
A hook that misses it is **Silent**, which the engine treats as a refusal
of the step it was serving. A reply with `ok: true` that lacks a field the
op requires is R0303, a contract violation, also a refusal. A reply with no
boolean `ok` is R0303.

## Ops by kind

| kind | op | request fields | reply fields |
|---|---|---|---|
| `journal` | `append` | `entry` (a journal entry) | |
| `inventory` | `list` | | `hosts`: `[{name, address, os, roles, reach, filesystem, stdin_preamble, scheduler, rue_root, artifact, facts}]` (Appendix C) |
| `execute` | `run` | `host`, `instance`, `body` (resolved primitives), `env`, `secrets` | `output: {stdout, outputs: {name: value}}`, `facts` |
| `execute` | `read_fact` | `host`, `shape` | `content` (the file's text, or absent) |
| `execute` | `bootstrap_state` | `host` | `state: {rue_root, group, instances_dir, lock, modes_ok}` |
| `execute` | `clock` | `host` | `epoch_s` (the host's own clock, for the arm-time skew probe, R0403) |
| `execute` | `instance_dir_create`, `instance_dir_remove` | `host`, `instance` | |
| `execute` | `instance_dir_list` | `host` | `dirs: [{instance, armed, fired, modes_ok}]` |
| `execute` | `put_file` | `host`, `instance`, `rel`, `content`, `mode` | |
| `execute` | `replace_file` | `host`, `instance`, `rel`, `content` | |
| `execute` | `get_file` | `host`, `instance`, `rel` | `content` |
| `execute` | `remove_file` | `host`, `instance`, `rel` | |
| `execute` | `host_lock` | `host` | (held until the next request on the host) |
| `probe` | `observe` | `host`, `probe` | `fact: {text, tri}` with `tri` one of `yes`, `no`, `unknown` |
| `approval` | `authenticators` | | `authenticators: [{id, human}]` |
| `approval` | `challenge` | `instance`, `digest`, `scope`, `context` | `challenge` |
| `approval` | `verify` | `instance`, `digest`, `scope`, `authenticator`, `proof` | `verified`, `reason` |
| `secrets` | `resolve` | `ref` | `value` |
| `secrets` | `deliver` | `instance`, `label`, `value` | `accepted`, `receipt` |
| `notify` | `deliver` | `level`, `subject`, `body` | |
| `scheduler` | `install`, `arm`, `rearm`, `disarm`, `present` | `host`, `artifact`, `deadline` | `present` (`true`, `false` or `"unknown"`) |

The `execute` ops beyond `run` are the instance-directory contract (7.7)
a hook that registers with `filesystem: true` must serve; a hook without a
filesystem is never asked them, and a `:target` undo on its host is refused
before `do` (R0408). `armed` is the artifact's presence in the directory
and `modes_ok` whether it carries the modes 7.7 requires: the engine reads
the first at reconciliation and the second when it arms (R0406).

The engine asks `inventory.list` **once**, as it starts: after the hook has
registered and before boot recovery, which needs the hosts to reconcile
against. So a hook that lists a site's hosts must be a spawned child
(`rued run --spawn`), since nothing has registered over the socket before
the daemon serves it, and a host added later needs a restart. A daemon
whose inventory hook does not answer refuses to start and says which hook:
booting with no hosts would report every plan unreachable, which reads as a
broken site rather than a missing hook. `rued run --inventory FILE` takes
the hosts from a record instead, which is how dry-run mode rehearses a
hook-inventoried site with no hook to ask.

A host a hook lists carries everything a `rue_toml()` inventory declares,
so an embedded site is not quietly less capable than a file-backed one.
`name` and `os` are required; the rest default. `rue_root` is where the
instance directory lives (7.7) and a run-capable host without one can hold
none; `stdin_preamble` defaults to `filesystem`; `artifact` is the language
a `:target` backstop is rendered in and defaults to the host's native
shell.

`execute.clock` is the one optional op: a hook that does not serve it
answers `ok: false`, and the engine records that no skew probe is possible
on that host rather than assuming the clocks agree.

The `scheduler` ops are the target-side entry that runs a rendered
artifact. `install` creates it, `disarm` removes it and `present` reports
it; `arm` and `rearm` carry a `deadline` for a scheduler that enforces the
time itself, and a scheduler whose entry is periodic (the artifact
comparing the `deadline` file the engine writes) has nothing to do in
them. A `present` of `"unknown"` is never read as absence.

A `body` in `execute.run` is the engine's resolved primitives: each a
JSON object with one key naming the primitive (`run`, `write`, `remove`,
`append`, `region_set`, `region_clear`, `stage`, `hook`, `install`,
`release`, `call`) whose values are `{text, secret}` pairs; a `secret:
true` value must never reach a command line, a log or a journal on the
hook's side.

## Secrets

A `Secret` crosses this boundary in exactly four messages: toward the hook
in `execute.run`'s body and in `secrets.deliver`; toward the engine in
`execute.run`'s `outputs` for an output the op declared secret and in
`secrets.resolve`'s `value`. No other request has a field a secret could
travel in; a hook that puts one in another reply has leaked it on its own
side, and the engine journals labels only, never values (R0305 names the
engine's refusal to send one elsewhere).

## Registration

```
{"register": {"name": "actuate", "kinds": ["execute", "probe"], "protocol": 1, "filesystem": false, "stdin_preamble": false}}
```

`name` is the hook the site's bindings name (`hook(:actuate, transport:
:api)`); `kinds` what it serves; `protocol` this document's version (R0501
otherwise). Registration is accepted only from a declared registrar
(R0505) and journaled `HookRegistered{name, registrar, connection}`. When
the connection ends the hook is deregistered and journaled; a request to
an unregistered hook is Silent for an executor and a refusal to
acknowledge (R0304) for a journal sink.

## A hook over stdio

A child (`rued run --spawn actuate="./my-hook"`) writes its `register`
frame as the first line of its stdout, reads the acknowledgement on its
stdin, then reads requests on stdin and writes replies on stdout, one line
each. It is the socket owner by construction and must still be a declared
registrar's hook.
