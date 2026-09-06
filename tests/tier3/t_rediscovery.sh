#!/bin/sh
# Self-test of the rediscovery table's guard: a listed patch that is missing,
# an unlisted patch, a patch whose target has moved, and a malformed row must
# each fail; an empty table must refuse; the real tree must pass.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/rediscovery/check-patches.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-rediscovery.XXXXXX") || exit 2
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

# The tree under test: the table, the patches, and the sources they touch.
reset_tree() {
    rm -rf "$tmp/tree"
    mkdir -p "$tmp/tree/tools/rediscovery/patches" "$tmp/tree/proto/src/Rue/Proto" "$tmp/tree/core/src" || exit 2
    cp "$root/tools/rediscovery/table.tsv" "$tmp/tree/tools/rediscovery/table.tsv"
    cp "$root"/tools/rediscovery/patches/*.patch "$tmp/tree/tools/rediscovery/patches/"
    cp "$root"/proto/src/Rue/Proto/*.hs "$tmp/tree/proto/src/Rue/Proto/"
    cp "$root"/core/src/*.rs "$tmp/tree/core/src/"
}

tab=$(printf '\t.')
tab=${tab%.}

# 0. The copied tree passes.
reset_tree
expect 0 "copied tree passes"

# 1. A row naming a patch that does not exist.
reset_tree
printf 'ghost.patch%s1%s-%scabal%slaws%s-\n' "$tab" "$tab" "$tab" "$tab" "$tab" >> "$tmp/tree/tools/rediscovery/table.tsv"
expect 1 "listed but missing patch is caught"

# 2. A patch file no row names.
reset_tree
cp "$tmp/tree/tools/rediscovery/patches/reach-late-arm.patch" "$tmp/tree/tools/rediscovery/patches/orphan.patch"
expect 1 "unlisted patch is caught"

# 3. A protection that moved: the patch's context is gone.
reset_tree
sed 's/armedBefore n = case planBackstop p of/armedBefore n = case (planBackstop p) of/' "$root/proto/src/Rue/Proto/Backstop.hs" > "$tmp/tree/proto/src/Rue/Proto/Backstop.hs"
expect 1 "patch whose target moved is caught"

# 4. A malformed row (five fields), and a row naming a suite that does not exist.
reset_tree
printf 'reach-late-arm.patch%s1%s-%scabal%sE0401\n' "$tab" "$tab" "$tab" "$tab" >> "$tmp/tree/tools/rediscovery/table.tsv"
expect 1 "malformed row is caught"
reset_tree
printf 'reach-late-arm.patch%s1%s-%spytest%sE0401%s-\n' "$tab" "$tab" "$tab" "$tab" "$tab" >> "$tmp/tree/tools/rediscovery/table.tsv"
expect 1 "unknown suite is caught"

# 5. An empty table refuses to check.
reset_tree
sed '/^[a-z]/d' "$root/tools/rediscovery/table.tsv" > "$tmp/tree/tools/rediscovery/table.tsv"
rm -f "$tmp/tree/tools/rediscovery/patches/"*.patch
expect 2 "empty table refuses with exit 2"

# 6. The real tree passes.
if sh "$guard" > "$tmp/out" 2>&1; then
    ok "the repository's patches all apply"
else
    bad "the repository's patches all apply"; cat "$tmp/out" >&2
fi

exit "$rc"
