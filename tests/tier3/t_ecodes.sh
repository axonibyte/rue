#!/bin/sh
# Self-test of the E-code guard: a code added on one side only must fail in
# either direction, a raw literal must fail, an unreadable source must refuse,
# and the real tree must agree.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-ecodes.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-ecodes.XXXXXX") || exit 2
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

reset_tree() {
    rm -rf "$tmp/tree"
    mkdir -p "$tmp/tree/proto/src/Rue/Proto" "$tmp/tree/core/src" "$tmp/tree/docs" || exit 2
    cp "$root/proto/src/Rue/Proto/Diagnostics.hs" "$tmp/tree/proto/src/Rue/Proto/Diagnostics.hs"
    cp "$root/core/src/diagnostics.rs" "$tmp/tree/core/src/diagnostics.rs"
    cp "$root/docs/ROADMAP.md" "$tmp/tree/docs/ROADMAP.md"
}

# 0. The real files, copied, agree.
reset_tree
expect 0 "copied tree agrees"

# 1. A code in the enum that the table lacks.
reset_tree
printf '  | E9999 -- planted\n' >> "$tmp/tree/proto/src/Rue/Proto/Diagnostics.hs"
expect 1 "enum-only code is caught"

# 2. A code in the table that the enum lacks: delete the enum's E0401 line.
reset_tree
sed '/^[[:space:]]*|[[:space:]]*E0401[^0-9]/d' "$root/proto/src/Rue/Proto/Diagnostics.hs" > "$tmp/tree/proto/src/Rue/Proto/Diagnostics.hs"
expect 1 "table-only code is caught"

# 3. A raw literal outside the enum.
reset_tree
mkdir -p "$tmp/tree/proto/app"
printf 'main = putStrLn "E0401"\n' > "$tmp/tree/proto/app/Planted.hs"
expect 1 "raw literal outside the enum is caught"

# 3b. The Rust enumeration is held to the table the same way.
reset_tree
printf '    E9999 => "planted",\n' >> "$tmp/tree/core/src/diagnostics.rs"
expect 1 "Rust-only code is caught"
reset_tree
sed '/^[[:space:]]*E0401[[:space:]]*=>/d' "$root/core/src/diagnostics.rs" > "$tmp/tree/core/src/diagnostics.rs"
expect 1 "table code missing from the Rust enumeration is caught"
reset_tree
mkdir -p "$tmp/tree/core/src"
printf 'fn planted() -> &%sstatic str { "E0401" }\n' "'" > "$tmp/tree/core/src/planted.rs"
expect 1 "raw literal in a Rust crate is caught"
reset_tree
: > "$tmp/tree/core/src/diagnostics.rs"
expect 2 "empty Rust enumeration refuses with exit 2"

# 4. A table with no codes is a refusal to check.
reset_tree
sed '/^### 6\.7 /,/^### 6\.8 /{/^| E[0-9]/d;}' "$root/docs/ROADMAP.md" > "$tmp/tree/docs/ROADMAP.md"
expect 2 "empty table refuses with exit 2"

# 5. The real tree agrees.
if sh "$guard" > "$tmp/out" 2>&1; then
    ok "the repository agrees"
else
    bad "the repository agrees"; cat "$tmp/out" >&2
fi

exit "$rc"
