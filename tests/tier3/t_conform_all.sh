#!/bin/sh
# Self-test of sdk/conform-all.sh's selection: naming SDKs judges those and
# no others, naming none judges all four, and a named SDK whose toolchain
# is absent fails rather than being skipped. The pipeline runs one SDK per
# image by name, so a selection that quietly judged nothing would be a
# green step that proved nothing.
#
# No toolchain is needed: the script runs from a copy in a scratch tree,
# against stub interpreters and a stub `rue` that records what it was
# asked to judge and judges it passed (or, in one case, failed).
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
shell=$(command -v sh) || exit 2

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-conform.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

tree=$tmp/tree
mkdir -p "$tree/sdk/python" "$tree/sdk/elixir" "$tree/sdk/java/src/main/java" \
    "$tree/sdk/dotnet/examples/ConformanceHook" "$tmp/tools" "$tmp/stubs" || exit 2
cp "$root/sdk/conform-all.sh" "$tree/sdk/" || exit 2

# The utilities the script itself uses, and nothing else: a PATH of only
# these is a machine with no toolchain at all.
for t in dirname mktemp rm mkdir find; do
    p=$(command -v "$t") || { echo "t_conform_all: no $t" >&2; exit 2; }
    ln -s "$p" "$tmp/tools/$t" || exit 2
done
for t in python3 elixir mix javac java dotnet; do
    printf '#!%s\nexit 0\n' "$shell" > "$tmp/stubs/$t" && chmod +x "$tmp/stubs/$t" || exit 2
done
printf '#!%s\nprintf "%%s\\n" "$*" >> "%s"\nexit 0\n' "$shell" "$tmp/judged" > "$tmp/rue" &&
    chmod +x "$tmp/rue" || exit 2
printf '#!%s\nexit 1\n' "$shell" > "$tmp/rue-fails" && chmod +x "$tmp/rue-fails" || exit 2

# conform(<path> <rue> <args...>): run the copy with PATH set to exactly
# <path>. DOTNET_ROOT is set so the script's /tank/cache/dotnet fallback,
# which the Ubuntu guest really has, cannot put a real SDK ahead of the stub.
conform() {
    path=$1; shift
    judge=$1; shift
    rm -f "$tmp/judged"
    : > "$tmp/judged"
    PATH=$path DOTNET_ROOT=$tmp/stubs "$shell" "$tree/sdk/conform-all.sh" \
        --rue "$judge" "$@" > "$tmp/out" 2>&1
    st=$?
}
judged() { wc -l < "$tmp/judged" | tr -d ' '; }
full=$tmp/stubs:$tmp/tools

# 1. Nothing named: all four judged.
conform "$full" "$tmp/rue"
if [ "$st" -eq 0 ] && [ "$(judged)" = 4 ] && grep -q 'every SDK conforms' "$tmp/out"; then
    ok "with no SDK named, all four are judged"
else bad "with no SDK named, all four are judged (exit $st, judged $(judged))"; cat "$tmp/out" >&2; fi

# 2. One named: that one and no other.
conform "$full" "$tmp/rue" java
if [ "$st" -eq 0 ] && [ "$(judged)" = 1 ] && grep -q ConformanceHook "$tmp/judged" &&
    ! grep -q '^== python' "$tmp/out"; then
    ok "naming java judges java alone"
else bad "naming java judges java alone (exit $st, judged $(judged))"; cat "$tmp/out" >&2; fi

# 3. Two named.
conform "$full" "$tmp/rue" python dotnet
if [ "$st" -eq 0 ] && [ "$(judged)" = 2 ] && grep -q conformance_hook.py "$tmp/judged" &&
    grep -q ConformanceHook.dll "$tmp/judged"; then
    ok "naming python and dotnet judges exactly those two"
else bad "naming python and dotnet judges exactly those two (exit $st, judged $(judged))"; cat "$tmp/out" >&2; fi

# 4. THE CASE THE SELECTION MUST NOT BREAK: a named SDK on a machine without
#    its toolchain fails, and its hook is never judged.
conform "$tmp/tools" "$tmp/rue" python
if [ "$st" -eq 1 ] && [ "$(judged)" = 0 ] && grep -q 'python needs python3' "$tmp/out"; then
    ok "a named SDK with no toolchain fails instead of being skipped"
else bad "a named SDK with no toolchain fails instead of being skipped (exit $st)"; cat "$tmp/out" >&2; fi

# 5. A named SDK that does not conform fails the run.
conform "$full" "$tmp/rue-fails" elixir
if [ "$st" -eq 1 ] && grep -q 'elixir does not conform' "$tmp/out"; then
    ok "a named SDK that does not conform fails"
else bad "a named SDK that does not conform fails (exit $st)"; cat "$tmp/out" >&2; fi

# 6. A name that is not an SDK is a usage error, not an empty selection
#    that judges nothing and passes.
conform "$full" "$tmp/rue" pyhton
if [ "$st" -eq 2 ] && [ "$(judged)" = 0 ]; then
    ok "an unknown SDK name is refused"
else bad "an unknown SDK name is refused (exit $st)"; cat "$tmp/out" >&2; fi

# 7. A named SDK whose directory is gone fails rather than passing having
#    judged nothing.
rm -rf "$tree/sdk/java"
conform "$full" "$tmp/rue" java
if [ "$st" -ne 0 ]; then
    ok "a named SDK with no directory fails"
else bad "a named SDK with no directory fails (exit 0)"; cat "$tmp/out" >&2; fi

exit "$rc"
