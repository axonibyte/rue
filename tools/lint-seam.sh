#!/bin/sh
# The seam guard (ROADMAP.md section 4.4).
#
# Greps framework code -- the Rust crates of ROADMAP.md section 4.2 (core/,
# render/, surface/, engine/, bindings/, cli/, daemon/), proto/ (minus its
# tenants/ sublibrary), tools/, ci/ and tests/ -- for the words in
# tools/seam-denylist.txt and fails on any hit. tenants/ and docs/ are not
# scanned: that is where tenant vocabulary belongs. Directories that do not
# exist yet are skipped; the crates arrive by phase.
#
# Deliberately dumb: it greps source as data, because a clever guard is one
# that can be reasoned around. Tier 3 of the testing methodology.
#
# Usage: lint-seam.sh [--root DIR] [--denylist FILE]
#
# Exit 0: clean.  Exit 1: at least one hit (printed).  Exit 2: the guard could
# not check anything (no denylist words, nothing to scan, grep error) -- which
# is louder than a pass because a guard that checks nothing is worse than one
# that fails.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
list=''

while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        --denylist) list=$2; shift ;;
        *) echo "usage: lint-seam.sh [--root DIR] [--denylist FILE]" >&2; exit 2 ;;
    esac
    shift
done
[ -n "$list" ] || list=$root/tools/seam-denylist.txt

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-seam.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

if [ ! -r "$list" ]; then
    echo "lint-seam: cannot read denylist $list" >&2
    exit 2
fi
# grep exits 1 when nothing survives the filter; the -s test below decides.
grep -v -e '^#' -e '^[[:space:]]*$' "$list" > "$tmp/words"
if [ ! -s "$tmp/words" ]; then
    echo "lint-seam: denylist $list has no words; the guard checked nothing" >&2
    exit 2
fi

dirs=''
for d in core render surface engine bindings cli daemon proto tools ci tests; do
    [ -d "$root/$d" ] && dirs="$dirs $d"
done
if [ -z "$dirs" ]; then
    echo "lint-seam: no framework directory exists under $root; nothing scanned" >&2
    exit 2
fi

cd "$root" || exit 2
# shellcheck disable=SC2086
#   Deliberate word splitting: $dirs is a space-separated list of directory
#   names being turned into one argument each.
grep -rnIiwF -f "$tmp/words" \
    --exclude-dir=tenants --exclude-dir=dist-newstyle --exclude-dir=target --exclude-dir=.git \
    --exclude=seam-denylist.txt \
    $dirs > "$tmp/hits" 2> "$tmp/err"
st=$?
case $st in
    0)
        echo "lint-seam: FAIL -- tenant or platform vocabulary in framework code:" >&2
        cat "$tmp/hits" >&2
        exit 1
        ;;
    1)
        echo "lint-seam: clean"
        exit 0
        ;;
    *)
        echo "lint-seam: grep failed (exit $st):" >&2
        cat "$tmp/err" >&2
        exit 2
        ;;
esac
