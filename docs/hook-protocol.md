# The hook protocol, version 1

How `rued` talks to a hook (docs/ROADMAP.md 7.5): newline-delimited JSON,
on the control socket after `hello` and `register`
(docs/control-protocol.md), or on the stdio of a child the daemon spawned.
The engine sends requests; the hook answers each by id. A hook serves the
kinds it registered.

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
| `inventory` | `list` | | `hosts`: `[{name, address, os, roles, reach, filesystem, scheduler, facts}]` (Appendix C) |
| `execute` | `run` | `host`, `instance`, `body` (resolved primitives), `env`, `secrets` | `output: {stdout, outputs: {name: value}}`, `facts` |
| `execute` | `read_fact` | `host`, `shape` | `content` (the file's text, or absent) |
| `execute` | `bootstrap_state` | `host` | `state: {rue_root, group, instances_dir, lock, modes_ok}` |
| `execute` | `instance_dir_create`, `instance_dir_remove` | `host`, `instance` | |
| `execute` | `instance_dir_list` | `host` | `dirs: [{instance, armed, fired}]` |
| `execute` | `put_file` | `host`, `instance`, `rel`, `content`, `mode` | |
| `execute` | `replace_file` | `host`, `instance`, `rel`, `content` | |
| `execute` | `get_file` | `host`, `instance`, `rel` | `content` |
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
before `do` (R0408).

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
