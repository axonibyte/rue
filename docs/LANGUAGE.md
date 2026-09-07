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
  execute via: [ssh(), hook(:bmc_api, transport: :api)]
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
declares it. What each binding means, and how the site's hosts, transports
and acceptors are derived from it and the inventory, is the resolver's
(unit B of Phase 2) and will be stated here when it lands.

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
message, and what was expected and found. The front end raises E0101 (a
parse error; at most one per line, so a recovered line cannot cascade)
and E0105 (the version marker) in this unit; the resolver's codes follow.
