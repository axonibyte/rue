#!/bin/sh
# Self-test of the golden hygiene guard: each violation it exists to catch is
# planted in a temporary tree and must be reported; a clean tree must pass;
# an empty tree must refuse with exit 2.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-goldens.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-goldens.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

exp=$tmp/tree/tenants/t0/expected/host-a
mkdir -p "$exp" || exit 2

expect() { # expect <status> <label>
    sh "$guard" --root "$tmp/tree" > "$tmp/out" 2>&1
    st=$?
    if [ "$st" -eq "$1" ]; then
        ok "$2"
    else
        bad "$2 (got $st, wanted $1)"; cat "$tmp/out" >&2
    fi
}

# 0. An empty tree is a refusal to check.
expect 2 "empty tree refuses with exit 2"

# 1. Clean files pass.
printf '{\n  "a": 1\n}\n' > "$exp/verdict.json"
printf 'plan on host-a: fully reversible (1 steps).\n' > "$exp/verdict.txt"
expect 0 "clean goldens pass"

# 2. Carriage return.
printf '{\r\n  "a": 1\r\n}\r\n' > "$exp/verdict.json"
expect 1 "carriage return is caught"
printf '{\n  "a": 1\n}\n' > "$exp/verdict.json"

# 3. Trailing whitespace.
printf 'plan on host-a: text \n' > "$exp/verdict.txt"
expect 1 "trailing whitespace is caught"

# 4. Missing final newline.
printf 'plan on host-a: text' > "$exp/verdict.txt"
expect 1 "missing trailing newline is caught"

# 5. Two trailing newlines.
printf 'plan on host-a: text\n\n' > "$exp/verdict.txt"
expect 1 "doubled trailing newline is caught"
printf 'plan on host-a: text\n' > "$exp/verdict.txt"

# 6. JSON not beginning with '{'.
printf '[\n  1\n]\n' > "$exp/verdict.json"
expect 1 "JSON golden not starting with a brace is caught"
printf '{\n  "a": 1\n}\n' > "$exp/verdict.json"

# 7. Empty file.
: > "$exp/explain.txt"
expect 1 "empty golden is caught"
rm -f "$exp/explain.txt"

expect 0 "restored tree passes again"

exit "$rc"
