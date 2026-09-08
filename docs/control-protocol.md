# The control protocol, version 1

The channel between `rue` (and any embedding host) and `rued`
(docs/ROADMAP.md 7.4). On unix a socket, mode `0660`, group `rue`; on
Windows a named pipe (`\\.\pipe\rue`) whose discretionary access-control
list grants SYSTEM and the local administrators full access and the same
group read and write, and no one else anything. Frames are
newline-delimited JSON objects; every line is one frame. The version is the
integer in the `hello`; `rued` refuses any other with R0501. Nothing above
the transport differs between the two.

## Identity

Identity comes from the operating system, never from the client. `rued`
reads the peer's effective uid (`SO_PEERCRED` on Linux, `LOCAL_PEERCRED`
on FreeBSD, `getpeereid` on macOS) or, on Windows, the client's SID by
impersonating the pipe; it maps that to an account name and matches the
name against the site's `operators` block:

```
operators do
  identity :ops_requester, user: "ops", operator_for: :all, admin: true, subscribe: [:breakglass]
  identity :reactive_host, user: :socket_owner, operator_for: [:shed_load]
end
```

- `user:` is the account, or `:socket_owner` for the account `rued` runs
  as, which on Windows is the account the service runs under.
- `operator_for:` is `:all` or the plan ids the identity may act on.
- `admin: true` grants the admin verbs and nothing about plan scope.
- `subscribe:` names the plans whose journal entries the connection
  receives as `event` frames.

There are no implicit operators: the socket owner and any member of group
`rue` are refused without a declaration (R0503). A user that maps to
several identities must name one in the `hello`. In daemon dry-run mode
(`rued run --dry-run`) a site with no `operators` block admits every peer as
the socket owner's `dry-run` identity, with admin.

## Frames

### hello

```
→ {"hello": {"proto": 1, "identity": "ops_requester"}}        identity optional
← {"hello": {"ok": true, "proto": 1, "identity": "ops_requester", "admin": true, "dry_run": false}}
← {"ok": false, "error": {"code": "R0503", "message": "..."}}   and the connection closes
```

The first frame of a connection must be the `hello`; anything else is
refused with a `protocol` error. R0501 is the version refusal.

### request and reply

```
→ {"id": 1, "verb": "apply", "args": {...}}
← {"id": 1, "ok": true, "result": {...}}
← {"id": 1, "ok": false, "error": {"code": "R0504", "message": "..."}}
```

Ids are the client's; replies carry them back. A reply to a verb that acts
on an instance is an outcome: `{"id", "state", "exit", "line"}`, the exit
code of section 6.8 and the verdict line the CLI prints last.

| verb | args | scope | result |
|---|---|---|---|
| `apply` | `ir` (the plan IR document), `params` (name to value), `acks` (step numbers acknowledged up front), `forced` (guard names), `mode` (`auto`, `manual`), `rehearsal` | the plan's id must be in `operator_for` (R0504) | outcome |
| `status` | `instance` (optional) | the instance's plan, or every plan in scope | a status record, or a list of them |
| `recant` | `instance`, `force` (names; `drift`, `unknown`) | the instance's plan | outcome |
| `renew` | `instance`, `wane_s` | | outcome |
| `confirm` | `instance` | | outcome |
| `commit` | `instance`, `reason` | | outcome |
| `resume` | `instance` | | outcome |
| `handoff_done` | `instance`, `step` | | outcome |
| `cancel` | `instance` | | outcome |
| `abandon` | `instance`, `reason` (required, non-empty) | admin (R0506) | outcome |
| `hooks` | | | the registered hook names |

Error codes: the R-codes of Appendix D where one applies (`R0101` is exit
75, `R0102`, `R0103`, `R0203`, `R0501`, `R0503`, `R0504`, `R0505`,
`R0506`); `refused` when the check refused the plan (the message is the
verdict prose; exit 1); `wrong_state` when the verb has no meaning in the
instance's state; `no_such_instance`; `protocol` for a malformed frame or
missing argument (exit 2).

### register

After `hello`, a connection may become a hook:

```
→ {"register": {"name": "authority", "kinds": ["approval"], "protocol": 1, "filesystem": false}}
← {"register": {"ok": true, "name": "authority"}}
← {"register": {"ok": false, "error": {"code": "R0505", "message": "..."}}}
```

Registration is accepted only from a peer that maps to a declared
registrar whose `may_register` names the hook (R0505); a hook is trusted
by name for everything it serves, so the registrar declaration is the
site's statement of that trust. From then on the daemon sends hook
requests on the same connection (docs/hook-protocol.md) and the connection
answers them; it may still send verbs as its operator identity (an
embedding host applies its own plans and serves its own hooks over one
connection). Registrations and disconnections are journaled
(`HookRegistered`, `HookDeregistered`, `OperatorConnected`,
`OperatorDisconnected`).

A child `rued` spawned (`--spawn NAME=COMMAND`) has no peer credentials and
is the socket owner by construction; its first line on stdout must be the
`register` frame, its stdin receives the acknowledgement and then the
requests, and it must still be a declared registrar's hook.

### event

A connection whose identity subscribed to a plan receives its journal
entries as they are chained:

```
← {"event": { ...the entry... }}
```

Events interleave with replies; a client reading a reply skips them.
Delivery is best-effort and never a journal refusal.

## The CLI

`rue apply|status|recant|renew|confirm|commit|resume|handoff-done|abandon|cancel`
speak this protocol. `--socket` (or `RUE_SOCKET`) names the socket;
`--identity` names the identity when the user maps to more than one. The
outcome's line is the last line on stdout; its exit code is the process's.
