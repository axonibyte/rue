#!/bin/sh
# The E-code guard (ROADMAP.md section 10, tier 3).
#
# Rue.Proto.Diagnostics (the prototype) and rue_core::diagnostics (the Rust
# crate) are the only places a code exists as text; the roadmap's section 6.7
# table is the one place a code is documented. This guard reads all three as
# data and requires each enumeration to agree with the table in both
# directions, and forbids a raw "E0xxx" string literal anywhere else in the
# prototype or the crates, so the enumerations stay the single sources.
#
# Usage: lint-ecodes.sh [--root DIR]
#
# Exit 0: agree.  Exit 1: a code is missing on one side, or a literal exists.
# Exit 2: nothing could be extracted from one side -- the guard checked
# nothing, which is louder than a pass.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-ecodes.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done

src=$root/proto/src/Rue/Proto/Diagnostics.hs
rs=$root/core/src/diagnostics.rs
doc=$root/docs/ROADMAP.md

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-ecodes.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

for f in "$src" "$rs" "$doc"; do
    if [ ! -r "$f" ]; then
        echo "lint-ecodes: cannot read $f" >&2
        exit 2
    fi
done

# Constructor lines: optional indent, '=' or '|', optional space, the code,
# then anything that is not another digit.
sed -n 's/^[[:space:]]*[|=][[:space:]]*\(E[0-9][0-9][0-9][0-9]\)\([^0-9].*\)\{0,1\}$/\1/p' "$src" > "$tmp/src"
# Rust arms of the codes! macro: optional indent, the code, optional space, '=>'.
sed -n 's/^[[:space:]]*\(E[0-9][0-9][0-9][0-9]\)[[:space:]]*=>.*/\1/p' "$rs" > "$tmp/rs"
# Table rows between the 6.7 and 6.8 headings.
sed -n '/^### 6\.7 /,/^### 6\.8 /s/^| \(E[0-9][0-9][0-9][0-9]\) |.*/\1/p' "$doc" > "$tmp/doc"

sort -u -o "$tmp/src" "$tmp/src"
sort -u -o "$tmp/rs" "$tmp/rs"
sort -u -o "$tmp/doc" "$tmp/doc"

if [ ! -s "$tmp/src" ]; then
    echo "lint-ecodes: no codes found in $src; the guard checked nothing" >&2
    exit 2
fi
if [ ! -s "$tmp/rs" ]; then
    echo "lint-ecodes: no codes found in $rs; the guard checked nothing" >&2
    exit 2
fi
if [ ! -s "$tmp/doc" ]; then
    echo "lint-ecodes: no codes found in $doc section 6.7; the guard checked nothing" >&2
    exit 2
fi

rc=0
comm -23 "$tmp/src" "$tmp/doc" > "$tmp/only-src"
comm -13 "$tmp/src" "$tmp/doc" > "$tmp/only-doc"
if [ -s "$tmp/only-src" ]; then
    echo "lint-ecodes: in Diagnostics.hs but not in ROADMAP.md section 6.7:" >&2
    cat "$tmp/only-src" >&2
    rc=1
fi
if [ -s "$tmp/only-doc" ]; then
    echo "lint-ecodes: in ROADMAP.md section 6.7 but not in Diagnostics.hs:" >&2
    cat "$tmp/only-doc" >&2
    rc=1
fi
comm -23 "$tmp/rs" "$tmp/doc" > "$tmp/only-rs"
comm -13 "$tmp/rs" "$tmp/doc" > "$tmp/only-doc-rs"
if [ -s "$tmp/only-rs" ]; then
    echo "lint-ecodes: in core/src/diagnostics.rs but not in ROADMAP.md section 6.7:" >&2
    cat "$tmp/only-rs" >&2
    rc=1
fi
if [ -s "$tmp/only-doc-rs" ]; then
    echo "lint-ecodes: in ROADMAP.md section 6.7 but not in core/src/diagnostics.rs:" >&2
    cat "$tmp/only-doc-rs" >&2
    rc=1
fi

# No literal codes outside the enum.
: > "$tmp/lits"
for d in proto/src proto/tenants proto/app proto/test; do
    [ -d "$root/$d" ] || continue
    grep -rnE --include='*.hs' --exclude=Diagnostics.hs '"E[0-9]{4}' "$root/$d" >> "$tmp/lits"
done
for d in core render surface engine bindings cli daemon tenants/harness; do
    [ -d "$root/$d" ] || continue
    grep -rnE --include='*.rs' --exclude=diagnostics.rs --exclude-dir=target '"E[0-9]{4}' "$root/$d" >> "$tmp/lits"
done
if [ -s "$tmp/lits" ]; then
    echo "lint-ecodes: raw code literals outside the enumerations (use the constructors):" >&2
    cat "$tmp/lits" >&2
    rc=1
fi

if [ "$rc" -eq 0 ]; then
    n=$(wc -l < "$tmp/src")
    echo "lint-ecodes: agree ($n codes)"
fi
exit "$rc"
