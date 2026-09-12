/**
 * rue, for editors (docs/ROADMAP.md section 6; docs/issues/0005).
 *
 * This grammar exists to highlight and to navigate, not to judge: `rue
 * check` is the authority on what a text means, and every rule here is a
 * shape the Rust front end also accepts. The guard in `tests/` parses
 * every tenant text and every valid corpus snippet with this grammar and
 * requires no ERROR node, so the two cannot drift apart in silence.
 *
 * Two things shape it. Statements end at newlines, so the extras below
 * carry only spaces, comments and a line continuation, and every statement
 * rule ends at a newline of its own. And keywords are contextual (section
 * 6.2): `user`, `content` and `window` are keyword-argument names as well
 * as keywords, so `word: $ => $.name` lets the generated lexer treat a
 * keyword as the identifier it is wherever the position does not call for
 * the keyword.
 */

const NL = /\r?\n/;

module.exports = grammar({
  name: 'rue',

  word: $ => $.name,

  extras: $ => [/[ \t]+/, $.comment],

  rules: {
    // A file is its version line, then declarations. `rue 0` is the first
    // line of every text and the parser refuses a file without it, which
    // is why it is in the grammar rather than in a comment.
    // Blank lines and comments before the version line are trivia; after
    // it, every statement's own terminator eats the blank lines that
    // follow it, so there is exactly one place a newline can be consumed.
    source_file: $ => seq(
      optional($._nl),
      $.version,
      repeat($._declaration),
    ),

    version: $ => seq('rue', field('version', $.integer), $._nl),

    // A comment runs to the end of its line. `#{` is never one: it opens
    // an interpolation, and a comment token that took it would swallow the
    // rest of the string it is inside, extras being matched between the
    // children of a structural rule.
    comment: _ => token(seq('#', optional(/[^{\n][^\n]*/))),

    _nl: _ => repeat1(NL),

    _declaration: $ => choice(
      $.site,
      $.import,
      $.defprobe,
      $.defprim,
      $.defop,
      $.defplan,
      $.defrole,
      $.defprotocol,
      $.defimpl,
    ),

    // --- the site block (6.3) ---------------------------------------------

    site: $ => seq('site', 'do', $._nl, repeat($._sitedecl), 'end', $._nl),

    _sitedecl: $ => choice(
      $.binding,
      $.max_wait,
      $.skew_tolerance,
      $.operators,
      $.hooks,
    ),

    binding: $ => seq(
      field('slot', $.binding_slot),
      field('value', $.bindexpr),
      repeat(seq(',', $.kwarg)),
      $._nl,
    ),

    binding_slot: $ => choice(
      seq('inventory', 'from:'),
      seq('journal', 'to:'),
      seq('approval', 'via:'),
      seq('secrets', 'from:'),
      seq('secrets', 'deliver_to:'),
      seq('notify', 'via:'),
      seq('execute', 'via:'),
      seq('backstop', 'scheduler:'),
    ),

    bindexpr: $ => choice($.call, $.list),

    max_wait: $ => seq('max_wait', $.duration, $._nl),
    skew_tolerance: $ => seq('skew_tolerance', $.duration, $._nl),

    operators: $ => seq('operators', 'do', $._nl,
      repeat($.identity), 'end', $._nl),

    identity: $ => seq('identity', field('name', $.atom),
      repeat1(seq(',', $.kwarg)), $._nl),

    hooks: $ => seq('hooks', 'do', $._nl,
      repeat($.registrar), 'end', $._nl),

    registrar: $ => seq('registrar', field('name', $.atom),
      repeat1(seq(',', $.kwarg)), $._nl),

    // --- imports ----------------------------------------------------------

    import: $ => seq('import', field('path', $.string),
      optional(seq('as', field('alias', $.name))), $._nl),

    // --- probes and primitives --------------------------------------------

    defprobe: $ => seq('defprobe', field('name', $.atom),
      optional($.params),
      'do', $._nl, repeat($._probeline), 'end', $._nl),

    _probeline: $ => choice(
      $.probe_run,
      $.probe_hook,
      $.probe_locus,
      $.probe_equivalence,
      $.probe_produces,
      $.probe_reads,
      $.probe_static,
    ),

    probe_run: $ => seq('run', $.string, $._nl),
    probe_hook: $ => seq('hook', $.atom, $._nl),
    probe_locus: $ => seq('locus', $.atom, $._nl),
    probe_equivalence: $ => seq('equivalence', $.atom, $._nl),
    probe_produces: $ => seq('produces', $.factshape, repeat(seq(',', $.factshape)), $._nl),
    probe_reads: $ => seq('reads', $.factshape, $._nl),
    probe_static: $ => seq('static', $.boolean, $._nl),

    defprim: $ => seq('defprim', field('name', $.atom), optional($.params),
      'do', $._nl,
      'run', $.string, optional(seq(',', $.kwarg)), $._nl,
      'end', $._nl),

    // --- ops --------------------------------------------------------------

    defop: $ => seq('defop', field('name', $.atom), ',', field('pattern', $.pattern),
      optional($.params), 'do', $._nl,
      repeat($._opline), 'end', $._nl),

    // A parameter declaration is written exactly as a keyword argument --
    // `ack: ack`, `drift: :defer` -- and the lexer sees one token for the
    // name and its colon either way, so there is one rule for both.
    params: $ => repeat1(seq(',', $.kwarg)),

    _opline: $ => choice(
      $.footprint,
      $.reach,
      $.pre,
      $.do_line,
      $.undo_line,
      $.undo_pre,
      $.post,
      $.undo_locus,
      $.refusal,
      $.drift,
      $.outputs,
      $.exclusivity,
      $.locus_line,
      $.handoff_done,
      $.suspend,
      $.reestablish,
    ),

    footprint: $ => seq('footprint',
      optional(seq($.fpentry, repeat(seq(',', $.fpentry)))), $._nl),

    // The kind carries its colon, because the lexer reads `owned:` as one
    // token wherever a keyword argument could stand -- which is every
    // position a name and a colon share (6.2, keywords are contextual).
    fpentry: $ => seq(field('kind', $.kind), field('shape', $.factshape)),

    kind: _ => token(prec(1, choice(
      'owned:', 'region:', 'modified:', 'derived:', 'append_only:', 'held:',
    ))),

    reach: $ => seq('reach', $._expr, repeat(seq(',', $._expr)), $._nl),
    pre: $ => seq('pre', $.guard, repeat(seq(',', $.guard)), $._nl),
    post: $ => seq('post', $.guard, repeat(seq(',', $.guard)), $._nl),
    undo_pre: $ => seq('undo_pre', $.factshape, repeat(seq(',', $.factshape)), $._nl),

    do_line: $ => seq('do:', field('body', $.body), $._nl),
    undo_line: $ => seq('undo:', field('undo', $.undo), $._nl),
    undo_locus: $ => seq('undo_locus:', $.atom, $._nl),
    drift: $ => seq('drift:', choice($.atom, $.name), $._nl),
    exclusivity: $ => seq('exclusivity:', $.atom, $._nl),
    locus_line: $ => seq('locus:', choice($.atom, $.call), $._nl),
    handoff_done: $ => seq('handoff_done:', $.call, $._nl),
    suspend: $ => seq('suspend:', $.body, $._nl),
    reestablish: $ => seq('reestablish:', $.body, $._nl),

    outputs: $ => seq('outputs', $.output, repeat(seq(',', $.output)), $._nl),
    output: $ => choice($.kwarg, $.name),

    refusal: $ => seq('refusal:', choice($.knell_refusal, $.hold_refusal, $.atom), $._nl),
    hold_refusal: $ => seq(':hold', optional(seq(',', $.kwarg))),
    knell_refusal: $ => seq('knell', repeat1(seq(',', $.kwarg))),

    undo: $ => choice(
      seq('compensate:', $.body),
      $.atom,
      $.body,
    ),

    // --- plans and items --------------------------------------------------

    defplan: $ => seq('defplan', field('name', $.atom), ',', field('pattern', $.pattern),
      optional($.params), 'do', $._nl,
      repeat($._planline), 'end', $._nl),

    _planline: $ => choice(
      $.gate,
      $.wane,
      $.backstop,
      $.plan_option,
      $.require_journal,
      $._item,
    ),

    gate: $ => seq('gate', $._expr, repeat(seq(',', $.kwarg)), $._nl),
    wane: $ => seq('wane', $.duration, repeat(seq(',', $.kwarg)), $._nl),
    backstop: $ => seq('backstop', 'trigger:', $.list, repeat(seq(',', $.kwarg)), $._nl),
    require_journal: $ => seq('require', 'journal:', $.atom, $._nl),

    // `fires_by_construction:`, `strictness:`, `mode:` and `exclusivity:`
    // are one shape: a name, a colon and a literal.
    plan_option: $ => seq(
      field('option', choice('fires_by_construction:', 'strictness:', 'mode:', 'exclusivity:')),
      field('value', $._expr), $._nl,
    ),

    _item: $ => choice(
      $.confirm,
      $.commit,
      $.par,
      $.slot,
      $.knell,
      $.preflight,
      $.observe,
      $.assert,
      $.repeat_count,
      $.repeat_over,
      $.when,
      $.pipeline,
      $.step,
    ),

    confirm: $ => seq('confirm', '(', ')', $._nl),
    commit: $ => seq('commit', '(', ')', $._nl),

    step: $ => seq(
      field('call', $.call),
      repeat(seq(',', $.kwarg)),
      optional(seq('as', field('alias', $.name))),
      $._nl,
    ),

    pipeline: $ => seq(
      $._step_head,
      repeat1(seq('|>', $._step_head)),
      $._nl,
    ),

    _step_head: $ => seq($.call, repeat(seq(',', $.kwarg)),
      optional(seq('as', field('alias', $.name)))),

    par: $ => seq('par', 'do', $._nl, repeat($._item), 'end', $._nl),
    slot: $ => seq('slot', $.atom, $._nl),
    knell: $ => seq('knell', $.step),

    preflight: $ => seq('preflight', 'do', $._nl,
      repeat(seq($.guard, $._nl)), 'end', $._nl),

    observe: $ => seq('observe', $.call, 'as', field('alias', $.name), $._nl),

    assert: $ => seq('assert', $.guard, repeat(seq(',', $.kwarg)), $._nl),

    repeat_count: $ => seq('repeat', $.integer, 'as', field('var', $.name), 'do', $._nl,
      repeat($._item), 'end', $._nl),

    // `max:` is required by the language and missing from E0106's negative
    // tenant, which is a text the front end parses and the checker refuses.
    // A grammar stricter than the parser would report a parse error where
    // rue reports a diagnostic, and an editor would underline the wrong
    // thing.
    repeat_over: $ => seq('repeat', 'over:', $._expr, optional(','),
      'as', field('var', $.name), optional(seq(',', 'max:', $.integer)),
      'do', $._nl, repeat($._item), 'end', $._nl),

    when: $ => seq('when', $.guard, repeat(seq(',', $.kwarg)), 'do', $._nl,
      repeat($._item),
      optional(seq('else', $._nl, repeat($._item))),
      'end', $._nl),

    // --- roles, protocols, impls -------------------------------------------

    defrole: $ => seq('defrole', field('name', $.atom), 'do', $._nl,
      repeat($.contribution), 'end', $._nl),

    contribution: $ => seq(field('slot', $.atom), optional(field('priority', $.integer)), $._item),

    defprotocol: $ => seq('defprotocol', field('name', $.atom),
      optional(seq(',', $.kwarg)), 'do', $._nl,
      repeat(seq('default', $._item)), 'end', $._nl),

    defimpl: $ => seq('defimpl', field('name', $.atom), ',', 'for:', field('role', $.atom),
      'do', $._nl, repeat($._item), 'end', $._nl),

    // --- patterns -----------------------------------------------------------

    pattern: $ => choice(
      seq($.record_pattern, optional(seq('=', field('bind', $.name)))),
      $.name,
      '_',
    ),

    record_pattern: $ => seq('%{',
      optional(seq($.pattern_pair, repeat(seq(',', $.pattern_pair)))), '}'),

    pattern_pair: $ => seq(field('key', $.kwarg_name), field('value', $._pat)),

    _pat: $ => choice($.atom, $.string, $.integer, $.name, $.list_pattern, '_'),

    list_pattern: $ => seq('[', optional(seq($._pat, repeat(seq(',', $._pat)))), ']'),

    // --- bodies -------------------------------------------------------------

    body: $ => choice($.list, $._prim),

    _prim: $ => $.call,

    // --- guards and expressions (6.5) ---------------------------------------

    guard: $ => choice(
      seq('force:', 'never', ',', $._expr),
      $._expr,
    ),

    _expr: $ => choice(
      $.binary,
      $.unary,
      $._expr_atom,
    ),

    binary: $ => choice(
      ...[
        ['or', 1], ['and', 2],
        ['==', 3], ['!=', 3], ['<', 3], ['<=', 3], ['>', 3], ['>=', 3],
        ['+', 4], ['-', 4],
        ['*', 5], ['/', 5], ['%', 5],
      ].map(([op, p]) => prec.left(p, seq(
        field('left', $._expr), field('operator', op), field('right', $._expr),
      ))),
    ),

    unary: $ => choice(
      prec(6, seq('not', $._expr)),
      prec(7, seq('-', $._expr)),
    ),

    _expr_atom: $ => choice(
      $.call,
      $.reference,
      $.string,
      $.atom,
      $.duration,
      $.float,
      $.integer,
      $.boolean,
      $.record,
      $.list,
      $.parenthesized,
    ),

    parenthesized: $ => seq('(', $._expr, ')'),

    call: $ => seq(
      field('name', $.qualified_name),
      '(',
      optional(seq(
        choice($.kwarg, $._expr),
        repeat(seq(',', choice($.kwarg, $._expr))),
      )),
      ')',
    ),

    // A body is a list or a call, and both are expressions: `content:` and
    // `do:` take the same shapes, so there is nothing to choose between.
    kwarg: $ => seq(field('name', $.kwarg_name), field('value', $._expr)),

    // `content:` and `user:` are written as one token, colon attached, so
    // a keyword argument cannot be confused with a name and a type.
    kwarg_name: _ => token(seq(/[a-z_][a-z0-9_]*/, ':')),

    qualified_name: $ => seq($.name, repeat(seq('.', $.name))),

    // A fact shape is a call or a dotted path: `file("/etc/x")`,
    // `bmc.account("bg")`, `record.placement`, `sshd_posture`.
    factshape: $ => choice($.call, $.reference),

    reference: $ => prec.left(seq($.name, repeat(seq('.', $.name)))),

    record: $ => seq('%{',
      optional(seq($.pair, repeat(seq(',', $.pair)))), '}'),

    pair: $ => seq(field('key', choice($.kwarg_name, seq(choice($.name, $.upper_name, $.string), ':'))),
      field('value', $._expr)),

    list: $ => seq('[', optional(seq(
      choice($.kwarg, $._expr), repeat(seq(',', choice($.kwarg, $._expr))),
    )), ']'),

    // --- tokens --------------------------------------------------------------

    name: _ => /[a-z_][a-z0-9_]*\??/,
    upper_name: _ => /[A-Z][A-Za-z0-9_]*/,

    atom: _ => token(choice(
      seq(':', /[a-z_][a-z0-9_]*\??/),
      seq(':"', /[^"]*/, '"'),
    )),

    duration: _ => token(seq(/[0-9]+/, choice('ms', 's', 'm', 'h', 'd'))),
    float: _ => token(seq(/[0-9]+/, '.', /[0-9]+/)),
    integer: _ => token(/[0-9]+/),
    boolean: _ => choice('true', 'false'),

    // A string is double-quoted with `#{...}` interpolation; the braces
    // inside it balance, so the interpolation is a nested node and not a
    // run of characters.
    string: $ => seq(
      '"',
      repeat(choice($.string_content, $.interpolation)),
      '"',
    ),

    string_content: _ => token.immediate(prec(1, /([^"\\#]|\\.|#[^{])+/)),

    interpolation: $ => seq('#{', $._expr, '}'),
  },
});
