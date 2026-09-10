#!/bin/sh
# The hook-protocol guard (ROADMAP.md section 10, tier 3).
#
# The protocol's ops exist in two places that must never drift: the table in
# docs/hook-protocol.md, which is what an SDK author reads, and `OPS` in
# hook-proto/src/op.rs, which is what the engine, the SDKs and
# `rue sdk-conform` execute. This guard reads both as data and requires them
# to agree in both directions.
#
# The third enumeration -- the cases of docs/sdk-conformance.md, as the
# runner actually drives them -- is bound to `OPS` by a test rather than by
# grep, because the cases are built from the constructors and not from
# literal strings: sdk/rust/tests/conform.rs requires the ops the suite
# drove to be exactly the ops of the table.
#
# Usage: lint-hook-ops.sh [--root DIR]
#
# Exit 0: the two agree.  Exit 1: an op is on one side only.  Exit 2:
# nothing could be extracted from one side -- the guard checked nothing,
# which is louder than a pass.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-hook-ops.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done

doc=$root/docs/hook-protocol.md
code=$root/hook-proto/src/op.rs
[ -r "$doc" ] || { echo "lint-hook-ops: no $doc" >&2; exit 2; }
[ -r "$code" ] || { echo "lint-hook-ops: no $code" >&2; exit 2; }

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-lint-hook-ops.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

# The document: rows of the ops table, whose first cell is a kind in
# backticks and whose second is one or more ops, comma separated.
awk -F '|' '
    /^\| *`[a-z_]+` *\| *`/ {
        kind = $2; gsub(/[` ]/, "", kind)
        ops = $3; gsub(/`/, "", ops)
        n = split(ops, a, ",")
        for (i = 1; i <= n; i++) {
            op = a[i]; gsub(/^[ \t]+|[ \t]+$/, "", op)
            if (op != "") print kind "." op
        }
    }
' "$doc" | sort -u > "$tmp/doc"

# The code: each element of OPS names its kind then its op.
awk '
    /^ *kind: "/ { kind = $0; sub(/^ *kind: "/, "", kind); sub(/".*$/, "", kind); next }
    /^ *op: "/   { op = $0; sub(/^ *op: "/, "", op); sub(/".*$/, "", op)
                   if (kind != "") print kind "." op; next }
' "$code" | sort -u > "$tmp/code"

[ -s "$tmp/doc" ] || { echo "lint-hook-ops: no ops read from $doc; the guard checked nothing" >&2; exit 2; }
[ -s "$tmp/code" ] || { echo "lint-hook-ops: no ops read from $code; the guard checked nothing" >&2; exit 2; }

rc=0
while read -r op; do
    grep -qx "$op" "$tmp/code" || {
        echo "lint-hook-ops: $op is documented in hook-protocol.md and absent from OPS" >&2
        rc=1
    }
done < "$tmp/doc"
while read -r op; do
    grep -qx "$op" "$tmp/doc" || {
        echo "lint-hook-ops: $op is in OPS and undocumented in hook-protocol.md" >&2
        rc=1
    }
done < "$tmp/code"

if [ "$rc" -eq 0 ]; then
    echo "lint-hook-ops: $(wc -l < "$tmp/code" | tr -d ' ') ops, documented and implemented"
fi
exit "$rc"
