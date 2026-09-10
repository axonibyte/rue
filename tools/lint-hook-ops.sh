#!/bin/sh
# The hook-protocol guard (ROADMAP.md section 10, tier 3).
#
# The protocol's ops exist in several places that must never drift: the
# table in docs/hook-protocol.md, which is what an SDK author reads; `OPS`
# in hook-proto/src/op.rs, which is what the engine and `rue sdk-conform`
# execute; and one transcription per SDK that cannot share the Rust table,
# starting with sdk/python. This guard reads them all as data and requires
# them to agree in every direction.
#
# An SDK's table is a transcription and not a copy of record: the Rust one
# is the source, and this is what stops a transcription rotting quietly
# while its own tests keep passing against its own idea of the protocol.
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

# Each SDK keeps its own transcription, because none of them can share the
# Rust table. Declared here as: <name> <directory> <file> <extractor>. The
# extractor prints one `kind.op` per line, reading a file whose newlines
# have been folded to spaces first -- a row may be written on one line or
# spread over several, and which one is a formatting choice the guard has
# no business having an opinion about.
#
# An SDK directory that exists must have a readable table: a leg quietly
# skipped is a guard reporting success for work it did not do.
sdk_table() { # sdk_table <name> <dir> <file> <pattern> <sed script>
    name=$1; dir=$root/$2; file=$root/$3; pattern=$4; script=$5
    [ -d "$dir" ] || return 0
    if [ ! -r "$file" ]; then
        echo "lint-hook-ops: $2 exists but $3 does not; the guard checked nothing" >&2
        exit 2
    fi
    tr '\n' ' ' < "$file" | grep -oE "$pattern" | sed -E "$script" | sort -u > "$tmp/$name"
    if [ ! -s "$tmp/$name" ]; then
        echo "lint-hook-ops: no ops read from $3; the guard checked nothing" >&2
        exit 2
    fi
    while read -r op; do
        grep -qx "$op" "$tmp/code" || {
            echo "lint-hook-ops: $op is in the $name SDK's table and absent from OPS" >&2
            rc=1
        }
    done < "$tmp/$name"
    while read -r op; do
        grep -qx "$op" "$tmp/$name" || {
            echo "lint-hook-ops: $op is in OPS and absent from the $name SDK's table" >&2
            rc=1
        }
    done < "$tmp/code"
}

# python: Op("kind", "op", ...)
sdk_table python sdk/python sdk/python/rue_hook/proto.py \
    'Op\( *"[a-z_]+", *"[a-z_]+"' \
    's/Op\( *"([a-z_]+)", *"([a-z_]+)"/\1.\2/'

# elixir: %Op{kind: "kind", op: "op", ...}
sdk_table elixir sdk/elixir sdk/elixir/lib/rue_hook/proto.ex \
    '%Op\{ *kind: *"[a-z_]+", *op: *"[a-z_]+"' \
    's/%Op\{ *kind: *"([a-z_]+)", *op: *"([a-z_]+)"/\1.\2/'

# java: row("kind", "op", ...) and new Op("kind", "op", ...)
sdk_table java sdk/java sdk/java/src/main/java/dev/rue/hook/Op.java \
    '(row|new Op)\( *"[a-z_]+", *"[a-z_]+"' \
    's/(row|new Op)\( *"([a-z_]+)", *"([a-z_]+)"/\2.\3/'

# dotnet: Row("kind", "op", ...) and new Op("kind", "op", ...)
sdk_table dotnet sdk/dotnet sdk/dotnet/src/RueHook/Op.cs \
    '(Row|new Op)\( *"[a-z_]+", *"[a-z_]+"' \
    's/(Row|new Op)\( *"([a-z_]+)", *"([a-z_]+)"/\2.\3/'

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
    echo "lint-hook-ops: $(wc -l < "$tmp/code" | tr -d ' ') ops, documented and implemented in every table"
fi
exit "$rc"
