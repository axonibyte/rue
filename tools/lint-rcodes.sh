#!/bin/sh
# The R-code guard (docs/ROADMAP.md section 10, tier 3).
#
# Appendix D is the one place a runtime code is documented. Every code it
# names must be raised by something a test runs: the engine's own sources
# say the words, and the suites assert them. This guard reads the appendix
# as data and requires every code in it to appear in the engine's, the
# bindings' or the daemon's sources, and to appear in at least one test,
# so a code cannot be documented and unreachable, nor raised and untested.
#
# Usage: lint-rcodes.sh [--root DIR]
#
# Exit 0: every code is raised and tested.  Exit 1: one is not.
# Exit 2: nothing could be extracted -- the guard checked nothing, which is
# louder than a pass.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-rcodes.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done

doc=$root/docs/ROADMAP.md
[ -r "$doc" ] || { echo "lint-rcodes: cannot read $doc" >&2; exit 2; }

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-rcodes.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

# The appendix's codes: every R0xxx between the Appendix D heading and the
# next heading.
awk '/^## Appendix D/ {on=1; next} on && /^## / {on=0} on' "$doc" |
    grep -o 'R0[0-9][0-9][0-9]' | sort -u > "$tmp/documented"
n=$(grep -c . "$tmp/documented" 2> /dev/null || echo 0)
[ "$n" -ge 20 ] || { echo "lint-rcodes: only $n codes found in Appendix D; the guard checked nothing" >&2; exit 2; }

# Where a code may be raised: the engine, the bindings, the daemon and the
# CLI. Where it may be asserted: any test under those crates, the tenants'
# harness, or the end-to-end harness.
sources=$(find "$root/engine/src" "$root/bindings/src" "$root/daemon/src" "$root/cli/src" \
    "$root/core/src" -name '*.rs' 2> /dev/null)
tests=$(find "$root/engine/tests" "$root/bindings/tests" "$root/cli/tests" "$root/core/tests" \
    "$root/sim" "$root/tenants" -name '*.rs' 2> /dev/null)
if [ -z "$sources" ] || [ -z "$tests" ]; then
    echo "lint-rcodes: no sources or no tests found" >&2
    exit 2
fi

rc=0
unraised=
untested=
while read -r code; do
    [ -n "$code" ] || continue
    # shellcheck disable=SC2086  # the file lists are deliberately split.
    if ! grep -q "$code" $sources 2> /dev/null; then
        unraised="$unraised $code"
        rc=1
        continue
    fi
    # shellcheck disable=SC2086  # likewise.
    if ! grep -q "$code" $tests 2> /dev/null; then
        untested="$untested $code"
        rc=1
    fi
done < "$tmp/documented"

if [ -n "$unraised" ]; then
    echo "lint-rcodes: FAIL -- documented but raised nowhere:$unraised" >&2
fi
if [ -n "$untested" ]; then
    echo "lint-rcodes: FAIL -- raised but asserted by no test:$untested" >&2
fi
[ "$rc" -eq 0 ] && echo "lint-rcodes: every documented runtime code is raised and tested ($n codes)"
exit "$rc"
