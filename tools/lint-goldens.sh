#!/bin/sh
# Golden hygiene (ROADMAP.md section 10, tier 3).
#
# Goldens are compared byte for byte by a later implementation, so their bytes
# are the contract: no carriage returns, no trailing whitespace, exactly one
# trailing newline, and a JSON golden begins with '{'. This guard reads them
# as data; it does not know what a verdict is.
#
# Usage: lint-goldens.sh [--root DIR]
#
# Exit 0: every golden is clean.  Exit 1: a violation (printed).  Exit 2: no
# golden files were found -- the guard checked nothing, which is worse than a
# failure.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-goldens.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-goldens.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

cd "$root" || exit 2
: > "$tmp/list"
for d in tenants docs; do
    [ -d "$d" ] || continue
    find "$d" -type d -name expected -exec find {} -type f \; >> "$tmp/list"
done
[ -f docs/state-transitions.tsv ] && echo docs/state-transitions.tsv >> "$tmp/list"
for f in docs/hook-protocol-v*.json; do
    [ -f "$f" ] && echo "$f" >> "$tmp/list"
done

if [ ! -s "$tmp/list" ]; then
    echo "lint-goldens: no golden files under $root; nothing checked" >&2
    exit 2
fi

cr=$(printf '\r')
rc=0
while IFS= read -r f; do
    if grep -q "$cr" "$f"; then
        echo "lint-goldens: $f: carriage return" >&2; rc=1
    fi
    if grep -nE '[[:space:]]+$' "$f" > "$tmp/ws"; then
        echo "lint-goldens: $f: trailing whitespace:" >&2
        cat "$tmp/ws" >&2; rc=1
    fi
    if [ ! -s "$f" ]; then
        echo "lint-goldens: $f: empty" >&2; rc=1
    else
        last=$(tail -c 1 "$f" | od -An -c | tr -d ' ')
        if [ "$last" != '\n' ]; then
            echo "lint-goldens: $f: no trailing newline" >&2; rc=1
        else
            tail2=$(tail -c 2 "$f" | od -An -c | tr -d ' ')
            if [ "$tail2" = '\n\n' ]; then
                echo "lint-goldens: $f: more than one trailing newline" >&2; rc=1
            fi
        fi
    fi
    case $f in
        *.json)
            first=$(head -c 1 "$f")
            if [ "$first" != '{' ]; then
                echo "lint-goldens: $f: JSON golden does not begin with '{'" >&2; rc=1
            fi
            ;;
    esac
done < "$tmp/list"

if [ "$rc" -eq 0 ]; then
    n=$(wc -l < "$tmp/list")
    echo "lint-goldens: clean ($n files)"
fi
exit "$rc"
