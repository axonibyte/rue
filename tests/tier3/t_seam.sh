#!/bin/sh
# Self-test of the seam guard: the guard must fail when a denylisted word is
# planted in framework code, pass when the same word sits under tenants/, and
# refuse to run against an empty denylist.
#
# The planted word is read out of the denylist so that this file never
# contains one of the words it polices.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-seam.sh
list=$root/tools/seam-denylist.txt

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-seam.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

word=$(sed -n '/^[^#[:space:]]/{p;q;}' "$list")
if [ -z "$word" ]; then
    bad "denylist has a first word to plant"
    exit 1
fi

mkdir -p "$tmp/tree/proto/src" "$tmp/tree/tools" "$tmp/tree/ci" "$tmp/tree/proto/tenants" || exit 2
printf 'module Clean where\n' > "$tmp/tree/proto/src/Clean.hs"
printf '#!/bin/sh\necho clean\n' > "$tmp/tree/tools/clean.sh"

# 1. A clean tree passes.
if sh "$guard" --root "$tmp/tree" --denylist "$list" > "$tmp/out" 2>&1; then
    ok "clean tree passes"
else
    bad "clean tree passes (exit $?)"; cat "$tmp/out" >&2
fi

# 2. A planted word in framework code fails with exit 1.
printf -- '-- planted: %s\n' "$word" > "$tmp/tree/proto/src/Planted.hs"
sh "$guard" --root "$tmp/tree" --denylist "$list" > "$tmp/out" 2>&1
st=$?
if [ "$st" -eq 1 ]; then
    ok "planted word in proto/ fails with exit 1"
else
    bad "planted word in proto/ fails with exit 1 (got $st)"; cat "$tmp/out" >&2
fi
rm -f "$tmp/tree/proto/src/Planted.hs"

# 3. The same word under a tenants/ directory is allowed.
printf -- '-- planted: %s\n' "$word" > "$tmp/tree/proto/tenants/Allowed.hs"
if sh "$guard" --root "$tmp/tree" --denylist "$list" > "$tmp/out" 2>&1; then
    ok "word under tenants/ is exempt"
else
    bad "word under tenants/ is exempt (exit $?)"; cat "$tmp/out" >&2
fi

# 4. Case-insensitive, whole-word: the upper-cased word is still a hit.
upper=$(printf '%s' "$word" | tr '[:lower:]' '[:upper:]')
printf '# %s\n' "$upper" > "$tmp/tree/ci/planted.sh"
sh "$guard" --root "$tmp/tree" --denylist "$list" > "$tmp/out" 2>&1
st=$?
if [ "$st" -eq 1 ]; then
    ok "matching is case-insensitive"
else
    bad "matching is case-insensitive (got $st)"; cat "$tmp/out" >&2
fi
rm -f "$tmp/tree/ci/planted.sh"

# 5. An empty denylist is a refusal to check, exit 2.
printf '# nothing here\n\n' > "$tmp/empty.txt"
sh "$guard" --root "$tmp/tree" --denylist "$tmp/empty.txt" > "$tmp/out" 2>&1
st=$?
if [ "$st" -eq 2 ]; then
    ok "empty denylist refuses with exit 2"
else
    bad "empty denylist refuses with exit 2 (got $st)"; cat "$tmp/out" >&2
fi

# 6. The real tree is clean right now.
if sh "$guard" > "$tmp/out" 2>&1; then
    ok "the repository is clean"
else
    bad "the repository is clean"; cat "$tmp/out" >&2
fi

exit "$rc"
