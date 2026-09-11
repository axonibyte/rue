#!/bin/sh
# Self-test of tools/rediscovery/run.sh itself, which asserts failures and
# so turns every way of not running a row into apparent success. Run against
# a scratch tree holding the runner, a table of its own, and a stub python3
# suite: every selected row must be judged, even when a suite reads stdin
# (mix under Elixir 1.18 did, and swallowed the rest of the row list); a
# protection whose removal nothing notices is reported; and a missing
# patch(1) or a missing toolchain refuses before any row runs.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
shell=$(command -v sh) || exit 2

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-rediscovery-run.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

tab=$(printf '\t.')
tab=${tab%.}

tree=$tmp/tree
mkdir -p "$tree/tools/rediscovery/patches" "$tree/sdk/python" "$tmp/stubs" "$tmp/tools" "$tmp/scratch" || exit 2
cp "$root/tools/rediscovery/run.sh" "$tree/tools/rediscovery/" || exit 2
printf 'protected\n' > "$tree/sdk/python/guard.txt"
printf 'unrelated\n' > "$tree/sdk/python/other.txt"

# Two rows whose patches revert the guard, and one whose patch changes
# something the suite never looks at.
revert() { # revert <file> <from> <to>
    printf -- '--- a/sdk/python/%s\n+++ b/sdk/python/%s\n@@ -1 +1 @@\n-%s\n+%s\n' "$1" "$1" "$2" "$3"
}
revert guard.txt protected reverted > "$tree/tools/rediscovery/patches/one.patch"
revert guard.txt protected reverted > "$tree/tools/rediscovery/patches/two.patch"
revert other.txt unrelated changed > "$tree/tools/rediscovery/patches/noticed-by-nothing.patch"
table() { # table <patch...>: a table holding a python row per patch
    printf 'patch-file%stier%sstage%ssuite%stest%senv\n' "$tab" "$tab" "$tab" "$tab" "$tab"
    for p in "$@"; do
        printf '%s.patch%s1%s-%spython%sguard%s-\n' "$p" "$tab" "$tab" "$tab" "$tab" "$tab"
    done
}

# The stub suite reads its stdin to the end first, as mix did, then passes
# while the guard is in place and fails once it is reverted. compileall, the
# build step, passes.
{
    printf '#!%s\n' "$shell"
    cat <<'STUB'
cat > /dev/null
case "$*" in *compileall*) exit 0 ;; esac
if grep -q protected guard.txt; then
    printf 'Ran 1 test in 0.001s\n\nOK\n'
    exit 0
fi
printf 'FAIL: test_guard (tests.Guard.test_guard)\nRan 1 test in 0.001s\n\nFAILED (failures=1)\n'
exit 1
STUB
} > "$tmp/stubs/python3" && chmod +x "$tmp/stubs/python3" || exit 2

# The utilities the runner uses: a PATH of these alone has no toolchain.
for t in awk cat cp dirname env grep mkdir mktemp patch rm sed sort tail tar tr wc; do
    p=$(command -v "$t") || { echo "t_rediscovery_run: no $t" >&2; exit 2; }
    ln -s "$p" "$tmp/tools/$t" || exit 2
done
nopatch=$tmp/nopatch
mkdir -p "$nopatch" || exit 2
for t in "$tmp"/tools/*; do
    [ "${t##*/}" = patch ] || ln -s "$(readlink "$t")" "$nopatch/${t##*/}" || exit 2
done

battery() { # battery <path> <args...>
    path=$1; shift
    PATH=$path TMPDIR=$tmp/scratch "$shell" "$tree/tools/rediscovery/run.sh" --tier 1 "$@" > "$tmp/out" 2>&1
    st=$?
}
full=$tmp/stubs:$tmp/tools

# 1. Two rows, a suite that reads stdin: both are judged, both rediscovered.
table one two > "$tree/tools/rediscovery/table.tsv"
battery "$full"
if [ "$st" -eq 0 ] && grep -q '^2 rediscovered, 0 not$' "$tmp/out"; then
    ok "every row is judged when a suite reads its stdin"
else bad "every row is judged when a suite reads its stdin (exit $st)"; cat "$tmp/out" >&2; fi

# 2. A protection whose removal nothing notices is not a rediscovery.
table one noticed-by-nothing > "$tree/tools/rediscovery/table.tsv"
battery "$full"
if [ "$st" -eq 1 ] && grep -q '^1 rediscovered, 1 not$' "$tmp/out" &&
    grep -q 'still passed with the protection reverted' "$tmp/out"; then
    ok "a patch nothing notices is reported, and fails the battery"
else bad "a patch nothing notices is reported, and fails the battery (exit $st)"; cat "$tmp/out" >&2; fi

# 3. No patch(1): refused before any row, not "the patch did not apply".
table one > "$tree/tools/rediscovery/table.tsv"
battery "$tmp/stubs:$nopatch"
if [ "$st" -eq 2 ] && grep -q 'patch (every row)' "$tmp/out" && ! grep -q '^==' "$tmp/out"; then
    ok "a missing patch(1) refuses before any row runs"
else bad "a missing patch(1) refuses before any row runs (exit $st)"; cat "$tmp/out" >&2; fi

# 4. No toolchain for the selected rows: refused, naming it.
battery "$tmp/tools"
if [ "$st" -eq 2 ] && grep -q 'python3 (python)' "$tmp/out" && ! grep -q '^==' "$tmp/out"; then
    ok "a missing toolchain refuses before any row runs"
else bad "a missing toolchain refuses before any row runs (exit $st)"; cat "$tmp/out" >&2; fi

# 5. A narrowing that selects nothing is a refusal, not an empty pass.
battery "$full" --suite cargo
if [ "$st" -eq 2 ] && grep -q 'no row in tier 1 matches --suite' "$tmp/out"; then
    ok "a --suite that selects nothing refuses"
else bad "a --suite that selects nothing refuses (exit $st)"; cat "$tmp/out" >&2; fi

# 6. A narrowed run says so in its summary.
table one two > "$tree/tools/rediscovery/table.tsv"
battery "$full" --row two
if [ "$st" -eq 0 ] && grep -q '^1 rediscovered, 0 not (tier 1 narrowed to row two)$' "$tmp/out"; then
    ok "a narrowed run names its narrowing"
else bad "a narrowed run names its narrowing (exit $st)"; cat "$tmp/out" >&2; fi

exit "$rc"
