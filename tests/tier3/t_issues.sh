#!/bin/sh
# Self-test of tools/lint-issues.sh: an issue with no status or a status the
# tracker does not know, a number that does not match its file, a number
# used twice, a closed issue naming no date and an open one naming one, a
# title with a pipe, a stray file, and an index that is missing a row, has
# a stale one or lists an issue that does not exist must each fail; a
# tracker with no index must refuse; a well-formed tracker and the real
# repository must pass.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-issues.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-issues.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

expect() { # expect <status> <label>
    sh "$guard" --root "$tmp/tree" > "$tmp/out" 2>&1
    st=$?
    if [ "$st" -eq "$1" ]; then
        ok "$2"
    else
        bad "$2 (got $st, wanted $1)"; cat "$tmp/out" >&2
    fi
}

d=$tmp/tree/docs/issues
issue() { # issue <file> <number> <title> <status> [closed]
    {
        printf '# %s: %s\n\n- status: %s\n- kind: defect\n- phase: 5\n- opened: 2026-09-11\n' "$2" "$3" "$4"
        [ $# -ge 5 ] && printf -- '- closed: %s\n' "$5"
        printf '\nA body.\n'
    } > "$d/$1"
}
reset_tree() {
    rm -rf "$tmp/tree"
    mkdir -p "$d" || exit 2
    issue 0001-first.md 0001 "The first" open
    issue 0002-second.md 0002 "The second" closed 2026-09-11
    printf '# Issues\n\n| # | Title | Kind | Status |\n|---|---|---|---|\n| 0001 | The first | defect | open |\n| 0002 | The second | defect | closed |\n' > "$d/README.md"
}
index_set() { # index_set <sed expression>: edit the README's index
    sed "$1" "$d/README.md" > "$tmp/r" && mv "$tmp/r" "$d/README.md"
}

reset_tree
expect 0 "a well-formed tracker passes"

reset_tree
sed '/^- status:/d' "$d/0001-first.md" > "$tmp/f" && mv "$tmp/f" "$d/0001-first.md"
expect 1 "an issue with no status is caught"

reset_tree
issue 0001-first.md 0001 "The first" pending
index_set 's/| The first | defect | open |/| The first | defect | pending |/'
expect 1 "a status the tracker does not know is caught"

reset_tree
issue 0001-first.md 0003 "The first" open
expect 1 "a number that does not match its file is caught"

reset_tree
issue 0001-again.md 0001 "The first" open
expect 1 "a number used twice is caught"

reset_tree
issue 0002-second.md 0002 "The second" closed
expect 1 "a closed issue naming no closing date is caught"

reset_tree
issue 0002-second.md 0002 "The second" closed abc1234
expect 1 "a closed issue naming something other than a date is caught"

reset_tree
issue 0001-first.md 0001 "The first" open 2026-09-11
expect 1 "an open issue naming a closing date is caught"

reset_tree
issue 0001-first.md 0001 "The | first" open
expect 1 "a title with a pipe is caught"

reset_tree
printf 'notes\n' > "$d/notes.txt"
expect 1 "a stray file in the tracker is caught"

reset_tree
index_set '/^| 0002 /d'
expect 1 "an issue missing from the index is caught"

reset_tree
index_set 's/| The second | defect | closed |/| The second | defect | open |/'
expect 1 "a stale status in the index is caught"

reset_tree
printf '| 0009 | Nobody | defect | open |\n' >> "$d/README.md"
expect 1 "an index row for an issue that does not exist is caught"

reset_tree
printf '# Issues\n\nNo index here.\n' > "$d/README.md"
expect 2 "a tracker with no index refuses with exit 2"

if sh "$guard" > "$tmp/out" 2>&1; then
    ok "the repository's tracker passes"
else
    bad "the repository's tracker passes"; cat "$tmp/out" >&2
fi

exit "$rc"
