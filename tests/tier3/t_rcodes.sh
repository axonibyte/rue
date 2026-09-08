#!/bin/sh
# Self-test of tools/lint-rcodes.sh: it fails on a documented code nothing
# raises, on a raised code no test asserts, and refuses to pass when it
# could read nothing.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
script=$root/tools/lint-rcodes.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-rcodes.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

# A tree of the shape the guard reads: an appendix, sources, tests.
mkdir -p "$tmp/docs" "$tmp/engine/src" "$tmp/bindings/src" "$tmp/daemon/src" \
    "$tmp/cli/src" "$tmp/core/src" "$tmp/engine/tests" "$tmp/bindings/tests" \
    "$tmp/cli/tests" "$tmp/core/tests" "$tmp/sim" "$tmp/tenants" || exit 2

appendix() {
    {
        echo "## Appendix D — Runtime codes"
        echo
        printf 'Codes:'
        # shellcheck disable=SC2016  # the backticks are the appendix's own
        # markup, not a substitution: the guard reads `R0xxx` as it is
        # written in docs/ROADMAP.md.
        for c in "$@"; do printf ' `%s`' "$c"; done
        echo ' .'
        echo
        echo "## Appendix E"
    } > "$tmp/docs/ROADMAP.md"
}

# Twenty codes, every one raised in a source and asserted in a test.
codes="R0101 R0102 R0103 R0104 R0201 R0202 R0203 R0204 R0301 R0302 R0303 R0304 R0305 R0401 R0402 R0403 R0404 R0405 R0406 R0407"
# shellcheck disable=SC2086  # the list is deliberately split into words.
appendix $codes
: > "$tmp/engine/src/lib.rs"
: > "$tmp/engine/tests/t.rs"
for c in $codes; do
    echo "// $c" >> "$tmp/engine/src/lib.rs"
    echo "// $c" >> "$tmp/engine/tests/t.rs"
done

if sh "$script" --root "$tmp" > "$tmp/out" 2>&1; then
    ok "a tree where every code is raised and tested passes"
else
    bad "a tree where every code is raised and tested passes"; cat "$tmp/out" >&2
fi

# A code documented and raised nowhere.
echo "// R0501" >> "$tmp/engine/tests/t.rs"
# shellcheck disable=SC2086
appendix $codes R0501
if sh "$script" --root "$tmp" > "$tmp/out" 2>&1; then
    bad "a code raised nowhere fails"
else
    if grep -q "raised nowhere" "$tmp/out"; then
        ok "a code raised nowhere fails"
    else
        bad "a code raised nowhere fails"; cat "$tmp/out" >&2
    fi
fi

# A code raised and asserted by no test.
echo "// R0501" >> "$tmp/engine/src/lib.rs"
grep -v R0501 "$tmp/engine/tests/t.rs" > "$tmp/t"
mv "$tmp/t" "$tmp/engine/tests/t.rs"
if sh "$script" --root "$tmp" > "$tmp/out" 2>&1; then
    bad "a code no test asserts fails"
else
    if grep -q "asserted by no test" "$tmp/out"; then
        ok "a code no test asserts fails"
    else
        bad "a code no test asserts fails"; cat "$tmp/out" >&2
    fi
fi

# Too few codes to have read the appendix at all: exit 2, not a pass.
appendix R0101 R0102
sh "$script" --root "$tmp" > "$tmp/out" 2>&1
if [ $? -eq 2 ]; then
    ok "an appendix it could not read refuses with exit 2"
else
    bad "an appendix it could not read refuses with exit 2"; cat "$tmp/out" >&2
fi

exit "$rc"
