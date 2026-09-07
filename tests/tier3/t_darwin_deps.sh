#!/bin/sh
# Self-test of the darwin dependency guard: a denied crate in the graph
# must fail, a clean graph must pass, an empty denylist or an unreadable
# listing must refuse, and the real tree must pass where cargo exists.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-darwin-deps.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-darwin.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

expect() { # expect <status> <label> <guard args...>
    st_want=$1; label=$2; shift 2
    sh "$guard" "$@" > "$tmp/out" 2>&1
    st=$?
    if [ "$st" -eq "$st_want" ]; then
        ok "$label"
    else
        bad "$label (got $st, wanted $st_want)"; cat "$tmp/out" >&2
    fi
}

printf 'rue v0.0.1 (/x/cli)\nserde v1.0.0\nlibc v0.2.0\n' > "$tmp/clean"
printf 'rue v0.0.1 (/x/cli)\nsecurity-framework-sys v2.0.0\nlibc v0.2.0\n' > "$tmp/dirty"

expect 0 "a std-only graph passes" --from "$tmp/clean"
expect 1 "a framework-linking crate is caught" --from "$tmp/dirty"
grep -q 'security-framework-sys' "$tmp/out" || bad "the hit is named"
: > "$tmp/empty"
expect 2 "an empty listing refuses with exit 2" --from "$tmp/empty"
expect 2 "an unreadable listing refuses with exit 2" --from "$tmp/nope"
printf '# nothing\n' > "$tmp/nolist"
expect 2 "an empty denylist refuses with exit 2" --from "$tmp/clean" --denylist "$tmp/nolist"

if command -v cargo > /dev/null 2>&1; then
    if sh "$guard" > "$tmp/out" 2>&1; then
        ok "the repository's darwin graph is clean"
    else
        bad "the repository's darwin graph is clean"; cat "$tmp/out" >&2
    fi
else
    echo "--      the repository's darwin graph: not run here (cargo absent; the darwin-deps phase is declared skipped on this host)"
fi

exit "$rc"
