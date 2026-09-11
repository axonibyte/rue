# Writing a command for the shim

## The contract

For each request, `rue-hook` runs `--command` through the host's shell
(`sh -c` on unix, `cmd /C` on Windows), writes the request to its stdin as
one line of JSON, and waits for it to exit:

- **An answer or a refusal** is a JSON object on the command's stdout:
  `{"ok":true, ...fields}` or `{"ok":false,"error":"why"}`. The last line
  counts, so a command may print progress before it. Leave out `id`; the
  shim sets it.
- **A command that fails** -- exits non-zero having written nothing, or
  writes a last line that is not a JSON object -- is answered for it:
  `ok: false`, naming the command and what went wrong.
- **Silence** is a command that exits 0 having written nothing. The shim
  sends no reply, and the engine reads the silence as a refusal of the step
  once its deadline passes. Because a command that merely forgot to print
  looks the same, the shim says so on its stderr each time.

A command runs once per request and holds no state between them unless it
keeps its own. Requests reach it in order, one at a time.

## The request

The request is the engine's frame as written in docs/hook-protocol.md,
compact and with its keys sorted, for example:

```text
{"entry":{"seq":1},"id":1,"kind":"journal","op":"append"}
{"host":"db-01","id":7,"kind":"probe","op":"observe","probe":"fenced"}
```

Sorted keys make a shell pattern such as `*'"kind":"journal"'*` reliable,
and `sed` can take a string field out (the conformance fixture,
`tests/fixtures/conformance-command.sh`, shows the idiom). For anything
richer, `jq` reads a request well.

## The reply each kind gives

The shim passes the command's object through, so the command must carry
every field its op requires; the engine refuses a reply that does not
(R0303). The ops and their fields are the table in docs/hook-protocol.md.
In brief:

| request | an answer carries |
|---|---|
| `journal.append`, `notify.deliver`, `scheduler.install`/`arm`/`rearm`/`disarm` | nothing beyond `"ok":true` |
| `inventory.list` | `"hosts":[{name, address, os, roles, reach, ...}]` |
| `execute.run` | `"output":{"stdout":..., "outputs":{...}}` |
| `probe.observe` | `"fact":{"text":..., "tri":"yes"\|"no"\|"unknown"}` |
| `approval.authenticators` / `challenge` / `verify` | `"authenticators":[{id, human}]` / `"challenge":...` / `"verified":bool, "reason":...` |
| `secrets.resolve` / `deliver` | `"value":...` / `"accepted":bool, "receipt":...` |
| `scheduler.present` | `"present":true`, `false` or `"unknown"` |

The instance-directory ops of `execute` (`read_fact`, `bootstrap_state`,
`clock`, `put_file`, ...) are sent only to a hook registered with
`--filesystem`; declare it only when the command answers all of them. A
`read_fact` answered `{"ok":true}` with no `content` means *no such file*,
while `"content":""` is a file that exists and is empty. A journal sink
that refuses an entry stops the plan (R0304), so refuse only when you
really did not record it; and never guess `scheduler.present`, since the
engine reads `false` as "install it again".

## Secrets

A secret reaches the command only on its stdin, inside the request line of
an `execute.run` (a resolved value is `{"secret":true,"text":"..."}`) or a
`secrets.deliver`; the shim never puts a request on a command line, where
every user of the host could read it. What happens next is the command's
business: pass a secret on to a child's stdin or environment, or a
mode-0600 file, and never onto a command line of its own. A secret the
command returns (`secrets.resolve`'s `value`, a secret output of a run)
goes back the same way.

## Options

- `--name NAME`: the name to register as.
- `--kinds k1,k2`: the kinds this hook serves; an unknown kind is refused
  at startup.
- `--command CMD`: the command each request is handed to.
- `--filesystem`: the command serves the instance-directory ops (7.7).
- `--stdin-preamble`: a run's `env:` and `stdin:` reach the command through
  a preamble on stdin.
