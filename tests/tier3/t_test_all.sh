#!/bin/sh
# Self-test of sdk/test-all.sh's selection, the sibling of t_conform_all.sh:
# naming SDKs runs those suites and no others, naming none runs all four, a
# named SDK whose toolchain is absent fails rather than being skipped, and a
# failing suite fails the run. The pipeline runs one SDK per image by name,
# so a selection that quietly ran nothing would be a green step that proved
# nothing.
#
# No toolchain is needed: the script runs from a copy in a scratch tree,
# against stub toolchains that record each invocation and pass (or, in one
# case, fail).
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
shell=$(command -v sh) || exit 2

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-test-all.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

tree=$tmp/tree
mkdir -p "$tree/sdk/python" "$tree/sdk/elixir" "$tree/sdk/java" "$tree/sdk/dotnet" \
    "$tmp/tools" "$tmp/stubs" "$tmp/failing" || exit 2
cp "$root/sdk/test-all.sh" "$tree/sdk/" || exit 2

# The utilities the script itself uses, and nothing else: a PATH of only
# these is a machine with no toolchain at all.
for t in dirname env; do
    p=$(command -v "$t") || { echo "t_test_all: no $t" >&2; exit 2; }
    ln -s "$p" "$tmp/tools/$t" || exit 2
done
# Each stub records the directory it ran in, its name and its arguments.
for t in python3 mix mvn dotnet; do
    {
        printf '#!%s\ntool=%s\nlog=%s\n' "$shell" "$t" "$tmp/ran"
        cat <<'STUB'
printf '%s %s %s\n' "${PWD##*/}" "$tool" "$*" >> "$log"
exit 0
STUB
    } > "$tmp/stubs/$t" && chmod +x "$tmp/stubs/$t" || exit 2
    ln -s "$tmp/stubs/$t" "$tmp/failing/$t" || exit 2
done
rm "$tmp/failing/mvn"
printf '#!%s\nexit 1\n' "$shell" > "$tmp/failing/mvn" && chmod +x "$tmp/failing/mvn" || exit 2

# suites(<path> <args...>): run the copy with PATH set to exactly <path>.
# DOTNET_ROOT is set so the script's /tank/cache/dotnet fallback, which the
# Ubuntu guest really has, cannot put a real SDK ahead of the stub.
suites() {
    path=$1; shift
    rm -f "$tmp/ran"
    : > "$tmp/ran"
    PATH=$path DOTNET_ROOT=$tmp/stubs "$shell" "$tree/sdk/test-all.sh" "$@" > "$tmp/out" 2>&1
    st=$?
}
ran() { wc -l < "$tmp/ran" | tr -d ' '; }
full=$tmp/stubs:$tmp/tools

# 1. Nothing named: all four suites run, each in its own directory. Elixir
#    is two invocations, the warnings-as-errors compile and the test run.
suites "$full"
if [ "$st" -eq 0 ] && [ "$(ran)" = 5 ] && grep -q 'every SDK suite passes' "$tmp/out" &&
    grep -q '^python python3 -m unittest discover' "$tmp/ran" &&
    grep -q '^elixir mix compile --warnings-as-errors' "$tmp/ran" &&
    grep -q '^elixir mix test --warnings-as-errors' "$tmp/ran" &&
    grep -q '^java mvn .*test' "$tmp/ran" && grep -q '^dotnet dotnet test tests/RueHook.Tests' "$tmp/ran"; then
    ok "with no SDK named, all four suites run"
else bad "with no SDK named, all four suites run (exit $st, ran $(ran))"; cat "$tmp/out" "$tmp/ran" >&2; fi

# 2. One named: that one and no other.
suites "$full" java
if [ "$st" -eq 0 ] && [ "$(ran)" = 1 ] && grep -q '^java mvn' "$tmp/ran" &&
    ! grep -q '^== python' "$tmp/out"; then
    ok "naming java runs java's suite alone"
else bad "naming java runs java's suite alone (exit $st, ran $(ran))"; cat "$tmp/out" >&2; fi

# 3. Two named.
suites "$full" python dotnet
if [ "$st" -eq 0 ] && [ "$(ran)" = 2 ] && grep -q '^python python3' "$tmp/ran" &&
    grep -q '^dotnet dotnet' "$tmp/ran"; then
    ok "naming python and dotnet runs exactly those two"
else bad "naming python and dotnet runs exactly those two (exit $st, ran $(ran))"; cat "$tmp/out" >&2; fi

# 4. THE CASE THE SELECTION MUST NOT BREAK: a named SDK on a machine without
#    its toolchain fails, and nothing runs.
suites "$tmp/tools" python
if [ "$st" -eq 1 ] && [ "$(ran)" = 0 ] && grep -q 'python needs python3' "$tmp/out"; then
    ok "a named SDK with no toolchain fails instead of being skipped"
else bad "a named SDK with no toolchain fails instead of being skipped (exit $st)"; cat "$tmp/out" >&2; fi

# 5. A failing suite fails the run, and the others still run.
suites "$tmp/failing:$tmp/tools"
if [ "$st" -eq 1 ] && grep -q 'java suite fails' "$tmp/out" && grep -q '^dotnet dotnet' "$tmp/ran" &&
    ! grep -q 'every SDK suite passes' "$tmp/out"; then
    ok "a failing suite fails the run"
else bad "a failing suite fails the run (exit $st)"; cat "$tmp/out" >&2; fi

# 6. A name that is not an SDK is a usage error, not an empty selection
#    that runs nothing and passes.
suites "$full" pyhton
if [ "$st" -eq 2 ] && [ "$(ran)" = 0 ]; then
    ok "an unknown SDK name is refused"
else bad "an unknown SDK name is refused (exit $st)"; cat "$tmp/out" >&2; fi

# 7. A named SDK whose directory is gone fails rather than passing having
#    run nothing.
rm -rf "$tree/sdk/java"
suites "$full" java
if [ "$st" -ne 0 ] && [ "$(ran)" = 0 ]; then
    ok "a named SDK with no directory fails"
else bad "a named SDK with no directory fails (exit $st)"; cat "$tmp/out" >&2; fi

exit "$rc"
