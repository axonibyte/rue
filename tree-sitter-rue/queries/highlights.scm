; Highlighting for rue (docs/ROADMAP.md section 6.6 names the keywords).
;
; The rule is that what rue treats as structure is highlighted as
; structure: an op's footprint kind, a plan's wane, a step's gate. Nothing
; here invents a category rue does not have.

; --- declarations ----------------------------------------------------------

[
  "site"
  "import"
  "as"
  "defprobe"
  "defprim"
  "defop"
  "defplan"
  "defrole"
  "defprotocol"
  "defimpl"
  "do"
  "end"
  "else"
] @keyword

"rue" @keyword.directive

; The declared name of anything is the name a reader looks for.
(defprobe name: (atom) @function)
(defprim name: (atom) @function)
(defop name: (atom) @function)
(defplan name: (atom) @function)
(defrole name: (atom) @type)
(defprotocol name: (atom) @type)
(defimpl name: (atom) @type)

; --- the site block --------------------------------------------------------

(binding_slot) @keyword
[
  "max_wait"
  "skew_tolerance"
  "operators"
  "hooks"
  "identity"
  "registrar"
] @keyword

; --- ops -------------------------------------------------------------------

[
  "footprint"
  "reach"
  "pre"
  "post"
  "undo_pre"
  "do:"
  "undo:"
  "undo_locus:"
  "drift:"
  "refusal:"
  "outputs"
  "exclusivity:"
  "locus:"
  "handoff_done:"
  "suspend:"
  "reestablish:"
  "compensate:"
  "knell"
] @keyword

(kind) @keyword.modifier

; --- plans and items -------------------------------------------------------

[
  "gate"
  "wane"
  "backstop"
  "trigger:"
  "require"
  "journal:"
  "fires_by_construction:"
  "strictness:"
  "mode:"
  "par"
  "slot"
  "preflight"
  "observe"
  "assert"
  "repeat"
  "over:"
  "max:"
  "when"
  "confirm"
  "commit"
  "default"
  "for:"
  "force:"
  "never"
] @keyword

; A probe's own lines.
[
  "run"
  "hook"
  "locus"
  "equivalence"
  "produces"
  "reads"
  "static"
] @keyword

"|>" @operator

; --- expressions -----------------------------------------------------------

[
  "or"
  "and"
  "not"
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "+"
  "-"
  "*"
  "/"
  "%"
  "="
] @operator

(call name: (qualified_name (name) @function.call))
(kwarg_name) @property
(pair key: (kwarg_name) @property)
(pattern_pair key: (kwarg_name) @property)

(reference (name) @variable)

(comment) @comment
(string) @string
(interpolation) @embedded
(atom) @constant
(integer) @number
(float) @number
(duration) @number
(boolean) @boolean
"_" @variable.builtin

[
  "("
  ")"
  "["
  "]"
  "%{"
  "}"
  "#{"
] @punctuation.bracket

[
  ","
  ":"
  "."
] @punctuation.delimiter
