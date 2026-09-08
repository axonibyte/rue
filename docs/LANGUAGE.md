# The rue language

What a `.rue` file may say, as the front end reads it. The rules of record
are `ROADMAP.md` section 6; this document is the reader's guide to them
and grows with the front end (Phase 2). Where it states a rule, section 6
states it too; where they differ, section 6 wins and this document is
wrong.

## A file

A file is UTF-8 with `\n` line endings and begins with the version marker
on its first line:

```
rue 0
```

`rue` and the language version this compiler reads (`0`). A file without
the marker, or with a newer version, is refused (E0105). After the marker
come definitions in any order: a `site do ... end` block, `import` lines,
and `defprobe`, `defprim`, `defop`, `defplan`, `defrole`, `defprotocol`
and `defimpl` definitions. Nothing else stands at the top level.

Comments begin with `#` and run to the end of the line. They may stand
alone on a line, at any indentation, or follow a statement. The parser
keeps every one; `fmt` writes them back where they were.

A statement ends at its newline. Brackets, parentheses and braces close on
the line that opened them; a statement is one line.

## Tokens

| Token | Spelling | Examples |
|---|---|---|
| name | `[a-z_][a-z0-9_]*`, optionally ending in `?` | `posture`, `host`, `defined?` |
| upper-case name | `[A-Z][A-Za-z0-9_]*`, a record key such as an environment variable | `BMC_PW` |
| atom | `:` and a name, or `:"..."` for characters a name cannot carry | `:freebsd`, `:"corpse:node-a"` |
| integer, number | `8443`, `1.5` | |
| duration | an integer and a unit, one token | `30m`, `60s`, `4h`, `1d`, `500ms` |
| boolean | `true`, `false` | |
| string | double-quoted; `#{expr}` interpolates; braces inside an interpolation balance | `"PermitRootLogin yes"`, `"cbsd bstart #{g}"` |

Keywords are contextual: `do`, `end`, `else`, `as`, `and`, `or`, `not` and
the statement keywords are names the parser reads by position, so a name
such as `user`, `content` or `window` also serves as a keyword-argument
name.

## Expressions

Precedence, loosest first: `or`; `and`; `not`; the comparisons `< <= > >=
== !=` (one per expression, never chained); `+ -`; `* / %`; unary `-`.
An atom of an expression is a literal, a reference (`name`, `host.address`,
`alias.output`), a call, a parenthesized expression, a record
`%{key: value, ...}` (keys are names, upper-case names or strings) or a
list `[a, b]`.

A call is a name, possibly qualified by dots, and its arguments in
parentheses: positional expressions first, then keyword arguments
`name: value`. `t3.pf_allow(port: 8443)` calls an imported definition;
`bmc.account("breakglass")` names a fact shape; the resolver tells them
apart by what the name is.

There are no user-defined functions. The builtins are `if/3`,
`defined?/1`, `unknown?/1`, `all_eq?/2`, `any_eq?/2`, `count_eq/2`,
`min/2`, `max/2`, `abs/1`, `to_s/1`, `to_s/2`, `len/1`, `member?/2` and
`secret/1`. `secret(:db_pw)` names a `secrets from:` binding; it is the
only way a secret enters a body.

## Keyword lines

Inside a `defop`, a `defplan` or a `site` block, most statements are a
keyword followed by arguments separated by commas:

```
footprint owned: file("/etc/x"), derived: sshd_posture
undo_pre file("/etc/x"), record.placement
backstop trigger: [after: 4h, unless_heartbeat: 60s, interval: 20s], locus: :target, arm_before: 3
refusal: knell, guard: verified_off, cost: fence_verdict(), ack: humans()
undo: compensate: [append(file("/var/log/x"), line: reversal)]
```

A keyword may be followed directly by a colon (`undo:`, `locus:`,
`mode:`); an argument is a keyword argument `name: value` or an
expression, and a keyword argument's value may itself be a keyword
argument (`undo: compensate: [...]`). A list may hold keyword arguments
(`[after: 4h, interval: 20s]`).

## Definitions

```
defprobe :sshd_posture do
  run "sshd -T"
  locus :target
  produces sshd_posture
end

defop :service_posture, %{os: :freebsd} do
  footprint owned: file("/etc/ssh/sshd_config.d/rue-breakglass.conf"), derived: sshd_posture
  do: [write(file("/etc/ssh/sshd_config.d/rue-breakglass.conf"), content: posture), run("service sshd reload")]
  undo: :restore
  post sshd_posture_applied
  undo_locus: :target
end

defplan :breakglass, %{name: "db-01"} do
  gate auth(:oncall), window: 30m
  wane 4h, renew_within: 30m
  service_posture(posture: "PermitRootLogin yes")
  bmc_account_enable() as bmc
end
```

A `defop` or `defplan` header is the definition's atom, a clause pattern,
and parameter declarations. The pattern is `_` (any host), a name, or a
record over host-contract facts, `%{os: :freebsd}`, `%{name: "db-01"}`,
`%{os: [:freebsd, :macos]}` (a list matches any member), optionally
captured with `= name`. Several definitions of one name with different
patterns are clauses, tried in file order for each host; the first match
wins. A parameter declaration is `name: name` for a required parameter or
`name: default` for one with a default (`defop :fence_corpse, _, ack: ack`;
`defop :shed_load, _, drift: drift`); a declared parameter stands wherever
the body would take a literal.

The lines a `defop` body may carry, in this order: `footprint` (may be
empty), `reach`, `pre`, `do:`, `undo:`, `undo_pre`, `post`, `undo_locus:`,
`refusal:`, `drift:`, `outputs`, `exclusivity:`, `locus:`, `handoff_done:`,
`suspend:` and `reestablish:`. The lines a `defplan` may open with, in any
order and each at most once: `gate`, `wane`, `backstop`, `fires_by_construction:`,
`strictness:`, `mode:`, `exclusivity:`, `require journal:`. Their meaning is
section 5's.

## Roles, protocols and primitives

A `defrole :db do ... end` contributes items to named slots: each line is
`:slot [priority] item`, priority 100 when absent. A plan's `slot :name`
is filled by every role whose atom is one of the host's roles (its
inventory `roles`), their contributions in order of priority, lower first,
ties by role name; a slot no role fills stays empty. A
`defprotocol :quiesce, inverse: :resume do default item end` names a
capability; `defimpl :quiesce, for: :db do items end` implements it for
a role; a step `quiesce()` expands to the impl for one of the host's
roles, else to the default. A protocol with an `inverse:` needs an impl
of the inverse for every role that implements it (E0103), and the
inverse must itself be a protocol (E0102). A `defprim :svc, name: name do
run "service #{name} restart", classes: %{name: :target_local} end`
declares a primitive: a call `svc(name: "sshd")` in a body is the run
template with the arguments substituted, each carrying the class the
declaration gives it (`:target_local` or `:controller`), which closure
reads.

## Items

A plan's body is a sequence of items: a step (`op_name(args) ... as alias`,
with `gate:`, `window:`, `on_lapse:` and `force: [...]` keyword arguments
after the call), a pipeline `a() |> b()`, `knell step`, `par do ... end`,
`slot :name`, `preflight do guards end`, `observe probe() as name`,
`assert guard` (`assert force: never, guard`), `repeat 3 as i do ... end`,
`repeat over: list, as x, max: 16 do ... end`, `when guard do ... else ... end`,
`confirm()` and `commit()`.

## The site block

```
site do
  inventory from: file("inventory.toml")
  journal to: local()
  approval via: hook(:authority)
  secrets deliver_to: [requester(), hook(:escrow)]
  execute via: [ssh(identity: "keys/id_ed25519", known_hosts: "known_hosts", user: "root"), hook(:bmc_api, transport: :api)]
  backstop scheduler: cron()
  max_wait 30m
  operators do
    identity :ops_requester, user: "ops", operator_for: :all, admin: true
  end
  hooks do
    registrar :authority, user: "approvald", may_register: [:authority, :escrow]
  end
end
```

A file's site is its own block when it has one, else the site of the one
file it imports; a path in a site resolves relative to the file that
declares it. The block is validated before anything is read from it: every binding is
a kind its line admits (E0601; `inventory from:` takes `rue_toml`, `file`
or `hook`; `journal to:` `file`, `stdout`, `local` or `hook`; `approval
via:` `always` or `hook`; `secrets from:` `file` or `hook`; `secrets
deliver_to:` `requester`, `hold` or `hook`; `notify via:` `stdout` or
`hook`; `execute via:` `local`, `ssh` or `hook`; `backstop scheduler:`
`cron`, `task_scheduler`, `launchd` or `hook`), each with the argument
its contract asks (E0602: a path string for `file`, `rue_toml` and `key`,
the hook's atom for `hook`, `until:` for `hold`, none for the rest,
`transport:` on an execute hook, and `identity:` and `known_hosts:` on
`ssh()`, which reads nothing under the daemon user's `~/.ssh`: `ssh(identity:
"keys/id_ed25519", known_hosts: "known_hosts", user: "root")`, paths
relative to the file that declares them), a journal is declared (E0603), an
operators block declares an identity (E0604), and every `hook(:x)` has a
registrar whose `may_register` names it (E0605).

At runtime a probe's `run` answers a guard by its exit status (0 is yes, 1
is no, anything else is unknown) and its stdout is the fact's value; a
`run` in an op binds a declared output with a line `rue-output NAME=VALUE`
on its stdout, which the executor removes from the run's text.

`approval via:` names the binding that publishes the authenticators a
gate may name, renders a challenge over a request digest, and verifies the
proofs that come back. `always()` opens every gate without a proof and
`rued` builds it only with `--dry-run`; everything else is `hook(:name)`.
`secrets deliver_to:` is a list, tried in order at the moment a producing
step completes: `requester()` takes the value only while a client is
attached and hands it to that client's reply, `hold(until: :wane |
DURATION)` keeps it in the daemon's memory until its bound and gives it up
once to `rue reveal`, and `hook(:name)` is anything else. A list every
acceptor declines is `applied; secret undelivered` and exit 7; a
`hold(until: :wane)` on a permanent plan resolves to the site's `max_wait`
and is R0104 where the site declares none. `notify via: stdout()` writes
one line per unbounded state on every reap pass, because what ends `Held`,
`Deferred`, `Stuck` and `DriftHeld` is a person.

`backstop scheduler:` names the binding that holds the target-side entry
for a rendered artifact, and every host the inventory marks as scheduled
uses it. `cron()` keeps one fenced region of the host's crontab, anchored
by the instance id and edited under the host lock, running the artifact
every minute; `task_scheduler()` keeps one scheduled task per instance;
`launchd()` keeps a job whose property list sits beside the artifact.
All three are periodic, and the deadline they honor is the `deadline`
file in the instance directory, which the engine writes when it arms and
rewrites when a renewal moves it: that is what the verdict means by
"self-enforced on `<host>`" and by cron's granularity of about a minute.
`hook(:name)` hands the same five ops to a site's own scheduler, which may
hold the time itself. `skew_tolerance` bounds how far a target's clock may
be from the controller's when the engine arms (R0403); with no line it is
120 seconds.

The `operators` block is the daemon's identity model (docs/control-protocol.md):
`identity :name, user: "account" | :socket_owner, operator_for: :all |
[:plan, ...], admin: true, subscribe: [:plan, ...]`. `user:` is required
(E0602): an identity is a statement about an OS user, which peer
credentials on the control socket are matched against; `:socket_owner`
is the account the daemon runs as. `operator_for:` scopes the plans the
identity may act on; `admin: true` grants the admin verbs and nothing
about plan scope; `subscribe:` names the plans whose journal entries the
connection receives. The `hooks` block declares who may register which
hook names: `registrar :name, user: ..., may_register: [:hook, ...]`,
`user:` likewise required. `journal to: file("j.ndjson"), sign:
key("journal_ed25519")` names the Ed25519 key (OpenSSH format) the daemon
signs every entry with; `rue journal verify --key` checks the public
half. The checker's site is
derived from the block and the inventory it names (a TOML file of `[[host]]` records and an
`[authenticators]` table, Appendix C):

| Site field | From |
|---|---|
| each host's name, os, reach, filesystem, artifact | the inventory record; `artifact` absent is the host's native shell (`sh`; `powershell` on Windows) |
| a host's `rue_root` | the inventory record's `rue_root`, else the family's default (`/var/db/rue`, `C:\ProgramData\rue`) |
| a host's stdin preamble | the record's `stdin_preamble`, else its `filesystem` |
| transports | the `execute via:` bindings: `ssh()` is `ssh`, `local()` is `local`, `hook(:x, transport: :t)` is `t`; with no line, `ssh` alone |
| authenticators | the inventory's `[authenticators]` table, in its order |
| max_wait | the `max_wait` line |
| scheduler presence | every host whose record has `scheduler` |
| secret acceptors | `secrets deliver_to:` in order: `requester()` is `requester`, `hook(:x)` is `hook:x`, `hold(...)` is `hold` |
| the requester | `--as`, else the first `identity` of `operators` |

## Resolution

`rue check file.rue --host H [--plan-name P] [--as ID]` (and `explain`,
`artifact`) resolves the file for one host into the same plan IR `rue
check` reads from a `plan.json`. `--host` names an inventory host; it may
be omitted when the plan has one clause whose pattern names the host
(`%{name: "db-01"}`). `--plan-name` names the plan when the file defines
more than one. The clauses of that name are tried in file order against
the host's contract facts and the first match is the plan (E0112 when none
matches; E0111 when a pattern names a fact that is not `name`, `os`,
`address`, `roles` or `reach`; E0103 when two clauses carry the same
pattern).

Each step's call expands the op's clauses the same way, against the plan's
host first and then against the host an `locus: host("...")` line names.
The call's keyword arguments bind the op's parameters: declared ones
(`ack: ack`, `drift: :defer`) and the free names its body uses. In the
body, a name's origin is what the classifier records: `host.<field>` is a
host field; `secret(:x)` is a secret; a name bound at the call to a
`repeat` variable or to a controller probe's fact is a controller value;
one bound to an earlier step's `alias.output` is that output; a fact a
`:target` probe produces is a fact; everything else is a parameter the
request binds. An output read before its step (or across a `par`
sibling) is E0110.

Fact shapes follow one rule: `file("/p")` is `file:/p`; `a.b` is `a:b`;
`a.b(x)` is `a:b:<x>` with a literal verbatim and a runtime value as
`{name}`; names are never rewritten; a probe under `derived:` is
`probe:<name>`; an anchor is the entry's, not part of the shape.

An op with no `undo:` line has no undo (E0201 unless it is a knell);
`undo: :restore` must be written. An undo is provably idempotent when it
is `:restore` or `compensate:`, or a computed body whose every `run` and
`hook` carries `idempotent: true` (E0208 otherwise). A step's arguments
are printed by `explain` as written, strings unquoted.

`repeat over: list, as x, max: N` needs its literal cap (E0106 without
it) and a set-valued list (a literal list with a repeated member is
E0113). A comparison against `:unknown` is E0108; ask `defined?()` or
`unknown?()`. An unknown op, probe or primitive is E0102 with the
nearest name suggested; an import that cannot be read or that cycles is
E0104.

## `rue fmt`

`rue fmt <file>` prints the file in the canonical layout; `--check` prints
nothing and exits 1 if the file is not already in it. The layout: two
spaces per block depth; one space after a comma and after a keyword's
colon; none inside brackets or around a dot; a call's parenthesis touches
its name; binary operators spaced; a trailing comment three spaces after
the statement; blank lines and comments as written; lines never rewrapped.
`fmt` is idempotent, is the identity on every tenant file, and refuses a
file with parse errors rather than rewrite it.

## Diagnostics

A diagnostic names the file, line and column, its code (section 6.7), a
message, and what was expected and found; on the command line it is
printed with the source line and a caret. The parser raises E0101 (a
parse error; at most one per line, so a recovered line cannot cascade)
and E0105 (the version marker); the resolver raises E0102, E0103, E0104,
E0106, E0107 (a call binding a declared parameter to a value of another
kind, or a plan option of the wrong kind), E0108, E0110, E0111, E0112,
E0113, E0114 (the arms of a `when` binding one alias to outputs of
different kinds), E0204 for a knell without a cost, and E0601 to E0605
for the site. Every one of them has a golden under `tenants/_negative/`
with its text and its rendered diagnostics. Every other code is the
checker's and reaches the verdict; E0109 is the renderer's.

## T3 by hand

Everything a commit-confirmed firewall change needs, from this document
alone. A site with an inventory of two firewalls, a journal, an approval
hook with its registrar, a scheduler, and an operator:

```
rue 0

site do
  inventory from: file("inventory.toml")
  journal to: local()
  approval via: hook(:authority)
  backstop scheduler: cron()
  operators do
    identity :netops_requester, user: "netops", operator_for: :all, admin: true
  end
  hooks do
    registrar :authority, user: "approvald", may_register: [:authority]
  end
end
```

A probe the plan observes after the change, run on the controller:

```
defprobe :verify_reach do
  run "true"
  locus :controller
  produces reach
end
```

Two clauses of one op, dispatched on the host's os. The FreeBSD one owns a
fenced region of `pf.conf` and restores it; the Windows one owns a firewall
rule and removes it by a run it declares idempotent. Both reach the host by
ssh and undo on the target, so a severed session still reverts:

```
defop :pf_allow, %{os: :freebsd} do
  footprint region: file("/etc/pf.conf", anchor: "rue-mgmt")
  reach ssh(host)
  do: [region_set(file("/etc/pf.conf", anchor: "rue-mgmt"), content: "pass in proto tcp to port #{port}"), run("pfctl -f /etc/pf.conf")]
  undo: :restore
  undo_locus: :target
end

defop :winfw_allow, %{os: :windows} do
  footprint owned: winfw.rule("rue-mgmt")
  reach ssh(host)
  do: run("New-NetFirewallRule -Name rue-mgmt -Direction Inbound -Protocol TCP -LocalPort #{port} -Action Allow")
  undo: run("Remove-NetFirewallRule -Name rue-mgmt", idempotent: true)
  undo_pre winfw.rule("rue-mgmt")
  undo_locus: :target
end
```

A permanent plan (it ends in `commit()`) with a backstop that fires unless
confirmed within ten minutes, armed before the change, one clause per os:

```
defplan :open_mgmt_port, %{os: :freebsd} do
  backstop trigger: [unless_confirmed: 10m], locus: :target, arm_before: 1
  pf_allow(port: 8443)
  observe verify_reach() as reach
  confirm()
  commit()
end

defplan :open_mgmt_port, %{os: :windows} do
  backstop trigger: [unless_confirmed: 10m], locus: :target, arm_before: 1
  winfw_allow(port: 8443)
  observe verify_reach() as reach
  confirm()
  commit()
end
```

`rue check plan.rue --host fw-01` then says: permanent; commits at step 4;
reversible through step 1; the backstop covers step 1 on the target, armed
before it; step 1 reverts unaided. `tenants/t3/plan.rue` is this file with
three more hosts.
