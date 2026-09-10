# The SDK conformance contract, version 1

`rue sdk-conform <command>` runs one hook and judges it against the
protocol of docs/hook-protocol.md. An SDK passes before it calls itself an
SDK (docs/ROADMAP.md 7.11).

The runner plays the daemon: it spawns `<command>` with `sh -c`, reads the
registration frame from its stdout, writes the acknowledgement to its
stdin, then sends one request per line and reads one reply per line. It
needs no daemon, no store and no plan. Exit 0 when every case passed, 1
when any failed, 2 when the hook could not be started or did not register.

```
rue sdk-conform --name conform 'python3 sdk/python/examples/conformance_hook.py'
rue sdk-conform --json --deadline-ms 2000 './my-hook serve'
```

Each SDK ships one, and two live in this repository as worked examples:
`sdk/rust/src/bin/conform_hook.rs` and
`sdk/python/examples/conformance_hook.py`. The `rue-hook` shim's is a
POSIX shell script, `sdk/shim/tests/fixtures/conformance-command.sh`, which
is the sharpest version of the claim: forty cases, eight kinds, no JSON
library.

## What a conformance hook is

A small program each SDK ships as an example, implementing the kinds that
SDK supports over the **fixed world below**. It is deliberately stateless:
every answer is a constant, because what is under test is the protocol and
the SDK's handling of it, not the hook's storage. A hook that serves only
some kinds is judged only on those, and every kind it does not serve must
still answer `ok: false` rather than go silent.

The world is spelled out so that five SDKs in five languages produce
byte-comparable answers, and so a sixth can be written from this file
alone.

### `inventory.list`

Exactly two hosts, in this order:

| field | `conform-full` | `conform-bare` |
|---|---|---|
| `name` | `conform-full` | `conform-bare` |
| `os` | `freebsd` | `linux` |
| `address` | `198.51.100.7` | *(omitted)* |
| `roles` | `["a", "b"]` | *(omitted)* |
| `reach` | `["hook"]` | *(omitted)* |
| `filesystem` | `true` | *(omitted)* |
| `stdin_preamble` | `false` | *(omitted)* |
| `scheduler` | `cron` | *(omitted)* |
| `rue_root` | `/var/db/rue` | *(omitted)* |
| `artifact` | `python` | *(omitted)* |
| `facts` | `{"site": "west"}` | *(omitted)* |

`conform-bare` carries only `name` and `os`, which is what proves an
omitted field takes its default rather than becoming an error.

### `execute`

| op | request | required answer |
|---|---|---|
| `run` | body of one `run` primitive, `cmd` = `conformance`, `env` `PW` = the secret `conformance-secret` | `output.stdout` = `ran 1 primitive\n`; `output.outputs.echo` = `conformance`; `output.outputs.secret` = `conformance-secret` |
| `read_fact` | `shape` = `file:/conformance/present` | `content` = `present\n` |
| `read_fact` | `shape` = `file:/conformance/absent` | **no** `content` field |
| `bootstrap_state` | | `state` with all five fields `true` |
| `clock` | | `epoch_s` = `1700000000`, **or** `ok: false` |
| `instance_dir_create`, `instance_dir_remove`, `remove_file`, `put_file`, `replace_file`, `host_lock` | | `ok: true`, no fields |
| `instance_dir_list` | | `dirs` = one entry, `conform-1`, armed, not fired, modes ok |
| `get_file` | `rel` = `deadline` | `content` = `1700000000\n` |

`execute.run`'s `outputs.secret` is the case that proves the handler
received the secret's real value: `execute.run` is one of the four
messages of 7.5 that may carry one, in both directions. An SDK that
scrubbed it on the way in, or could not reach it without formatting it
into a string, fails here.

`execute.clock` is the one op a hook may decline outright. Both answers
conform, and the runner says which was given.

### `probe.observe`

| `probe` | required answer |
|---|---|
| `conform-yes` | `fact` = `{"text": "yes", "tri": "yes"}` |
| `conform-no` | `fact` = `{"text": "no", "tri": "no"}` |
| `conform-unknown` | `fact` = `{"text": "", "tri": "unknown"}` |

And four **provocations**, which a conformance hook must implement exactly
as described. They are how an SDK demonstrates that its author can refuse
properly, and how the runner proves it detects what the engine would:

| `probe` | the hook must | the runner must judge it |
|---|---|---|
| `conform-refuse` | answer `ok: false` with a reason | a refusal, named |
| `conform-missing-field` | answer `{"id": N, "ok": true}` with no `fact` | **R0303** |
| `conform-no-ok` | answer `{"id": N}` with no boolean `ok` | **R0303** |
| `conform-silent` | not answer at all | **Silent**, at the deadline |

### `approval`

`authenticators` returns exactly `conform-human` (`human: true`) and
`conform-machine` (`human: false`), in that order.

`challenge` returns a string containing the digest it was given.

`verify` accepts exactly one proof, and it is bound to both the digest and
the scope it was made for (5.11). The proof that verifies is the text:

```
<digest>/plan          for scope "plan"
<digest>/step/<n>      for scope {"step": n}
<digest>/ack/<n>       for scope {"ack": n}
```

Anything else answers `verified: false` with a reason. The runner offers a
proof made for one digest and scope against two others, and a hook that
accepts either has no replay resistance and fails.

### `secrets`

| op | request | required answer |
|---|---|---|
| `resolve` | `ref` = `conform` | `value` = `conformance-resolved-secret` |
| `deliver` | `label` = `conform` | `accepted` = `true`, `receipt` = `receipt-conform` |
| `deliver` | `label` = `unwanted` | `accepted` = `false` |

Declining a delivery is not a refusal: the engine offers the secret to the
next acceptor, so `ok` is still `true`.

### `notify.deliver` and `scheduler`

`notify.deliver` answers `ok: true` with no fields.

`install`, `arm`, `rearm` and `disarm` answer `ok: true` with no fields.
`present` answers by artifact name: `conform-present.sh` is `true`,
`conform-absent.sh` is `false`, anything else is the string `"unknown"`.
Never guess: the engine reads `"unknown"` as "ask again", and `false` as
"install it again", and the second is destructive when it is wrong.

## What the runner checks on every reply

Beyond the case's own answer, every reply is judged against the op's row
in `rue-hook-proto`:

* the reply carries the `id` of its request;
* `ok` is a boolean (R0303 otherwise);
* an `ok: true` reply carries every field the op requires (R0303
  otherwise);
* a reply arrives within the deadline (Silent otherwise).

An op belonging to a kind the hook did not register, and any `kind.op`
pair the protocol has no row for, must be answered `ok: false`. Silence
there is a failure: the engine cannot tell "I do not serve that" from "I
am gone", and the operator is left with a step that did not happen and no
reason.

## The table this file and the code share

The protocol's ops are enumerated in three places, and all three are bound
to each other so none can drift:

* `tools/lint-hook-ops.sh` (a gate phase) reads the ops table of
  docs/hook-protocol.md and `OPS` in `rue-hook-proto` as data and requires
  them to agree in both directions.
* `sdk/rust/tests/conform.rs` requires the ops the suite actually drove to
  be exactly the ops of `OPS`. That leg is a test rather than a grep
  because the cases are built from the request constructors and not from
  literal strings: an op that gains a row and no case is a hole in the
  suite that nothing else would notice.

So an op added to the protocol fails the gate until it is documented here,
and fails the tests until it is driven.
