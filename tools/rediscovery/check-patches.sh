#!/bin/sh
# The rediscovery table's guard (a gate phase). Cheap and always on, unlike
# run.sh: every row's patch must exist and still apply to the working tree in
# a dry run, every row must be well formed, and every patch file must be
# listed. A refactor that moves a protected check makes its patch stop
# applying, and the gate fails until the patch is redone against the new
# shape -- so the battery can never silently rot into "the patch did not
# apply", which run.sh would report as a non-rediscovery only when someone
# remembers to run it.
#
# Usage: check-patches.sh [--root DIR]
#
# Exit 0: every row and patch is in order.  Exit 1: a row is malformed, a
# patch is missing, unlisted or no longer applies.  Exit 2: no rows at all,
# or patch(1) is missing -- the guard checked nothing.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: check-patches.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done
here=$root/tools/rediscovery
table=$here/table.tsv

tab=$(printf '\t.')
tab=${tab%.}

command -v patch > /dev/null 2>&1 || { echo "check-patches: patch(1) is not on PATH" >&2; exit 2; }
[ -r "$table" ] || { echo "check-patches: no table at $table" >&2; exit 2; }

# GNU patch dry-runs with --dry-run; FreeBSD patch with -C.
if patch --version 2>&1 | grep -q 'GNU patch'; then
    dry=--dry-run
else
    dry=-C
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-check-patches.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

awk -F "$tab" '
    /^#/ { next }
    /^[[:space:]]*$/ { next }
    $1 == "patch-file" { next }
    { print }
' "$table" > "$tmp/rows"

if [ ! -s "$tmp/rows" ]; then
    echo "check-patches: the table has no rows; a battery with nothing in it proves nothing" >&2
    exit 2
fi

# Two things about every hunk of a unified diff, checked here as
# arithmetic rather than by running patch(1), because which patch is
# installed decides which of them it forgives:
#
#   * the counts in `@@ -s,o +s,n @@` are the lines the hunk actually
#     carries. GNU patch reads them; FreeBSD's patch recounts and says
#     nothing, so a hand-edited header passes on the workstation and
#     fails on every GNU host.
#   * as many leading as trailing context lines, unless the hunk really
#     touches the file's start (old line 1) or end (its old range reaches
#     the file's last line, counted in the tree).
well_formed_hunks() { # well_formed_hunks <patch>
    awk -v root="$root" '
        function flush(   want_old, want_new) {
            if (!inhunk) return
            want_old = ctx + del
            want_new = ctx + add
            if (want_old != len || want_new != newlen) {
                printf "%s: hunk at %d declares -%d,%d +%d,%d but carries %d old and %d new lines\n", file, start, start, len, newstart, newlen, want_old, want_new
                bad = 1
            }
            if (lead != trail) {
                at_start = (start == 1)
                cmd = "wc -l < \"" root "/" file "\""
                cmd | getline total; close(cmd)
                at_end = (start + len - 1 == total + 0)
                if (!(lead < trail && at_start) && !(lead > trail && at_end)) {
                    printf "%s: hunk at %d has %d leading and %d trailing context lines\n", file, start, lead, trail
                    bad = 1
                }
            }
            inhunk = 0
        }
        /^\+\+\+ b\// { flush(); file = substr($0, 7); next }
        /^@@ / {
            flush()
            split($2, r, /[,]/); start = substr(r[1], 2) + 0
            len = (r[2] == "" ? 1 : r[2] + 0)
            split($3, n, /[,]/); newstart = substr(n[1], 2) + 0
            newlen = (n[2] == "" ? 1 : n[2] + 0)
            inhunk = 1; lead = 0; trail = 0; seen = 0
            ctx = 0; del = 0; add = 0
            next
        }
        inhunk && /^ / { ctx++; if (seen) trail++; else lead++; next }
        inhunk && /^-/ { del++; seen = 1; trail = 0; next }
        inhunk && /^\+/ { add++; seen = 1; trail = 0; next }
        inhunk && /^\\/ { next }
        { flush() }
        END { flush(); exit bad }
    ' "$1"
}

rc=0
: > "$tmp/listed"
while IFS="$tab" read -r c1 c2 c3 c4 c5 c6 extra; do
    if [ -z "$c1" ] || [ -z "$c2" ] || [ -z "$c3" ] || [ -z "$c4" ] || [ -z "$c5" ] || [ -z "$c6" ] || [ -n "${extra:-}" ]; then
        echo "check-patches: malformed row (need exactly 6 tab-separated fields): $c1" >&2
        rc=1
        continue
    fi
    case $c4 in
        cabal|cargo) ;;
        *) echo "check-patches: $c1: suite '$c4' is neither cabal nor cargo" >&2; rc=1 ;;
    esac
    case $c2 in
        *[!0-9]*|'') echo "check-patches: $c1: tier '$c2' is not a number" >&2; rc=1 ;;
    esac
    case $c1 in
        *.patch) ;;
        *) echo "check-patches: $c1: patch-file must end in .patch" >&2; rc=1 ;;
    esac
    echo "$c1" >> "$tmp/listed"
    if [ ! -r "$here/patches/$c1" ]; then
        echo "check-patches: $c1: listed but not present under tools/rediscovery/patches/" >&2
        rc=1
        continue
    fi
    # Both of the shapes FreeBSD's patch forgives and GNU's does not: a
    # hunk header whose counts do not match what the hunk carries, and one
    # with more leading than trailing context (to GNU patch, a hunk that
    # reaches the end of its file; the reverse, the start), which is tried
    # there and nowhere else. Either passes on the workstation and fails on
    # every GNU host, so the guard refuses the shapes themselves.
    if ! well_formed_hunks "$here/patches/$c1" > "$tmp/out" 2>&1; then
        echo "check-patches: $c1: a hunk is malformed; GNU patch would refuse it or anchor it to the file's edge" >&2
        sed 's/^/        /' "$tmp/out" >&2
        rc=1
        continue
    fi
    # No fuzz: a patch whose context has drifted is a protection that moved.
    if ( cd "$root" && patch -p1 -F 0 -s "$dry" < "$here/patches/$c1" ) > "$tmp/out" 2>&1; then
        echo "ok      $c1 applies"
    else
        echo "check-patches: $c1 no longer applies to the tree; the protection it reverts has moved" >&2
        sed 's/^/        /' "$tmp/out" >&2
        rc=1
    fi
done < "$tmp/rows"

for f in "$here"/patches/*.patch; do
    [ -f "$f" ] || continue
    b=$(basename "$f")
    if ! grep -qx "$b" "$tmp/listed"; then
        echo "check-patches: $b exists but no table row names it" >&2
        rc=1
    fi
done

exit "$rc"
