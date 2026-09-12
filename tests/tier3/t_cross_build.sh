#!/bin/sh
# Self-test of the cross-build guard: it must fail on a workspace member
# that compiles C and is not excluded, fail on an exclusion naming nothing,
# pass when the two agree, and refuse to run against a tree it cannot read.
#
# The trees are built here rather than borrowed from the repository, so the
# test says what it tests and the repository's own answer is a separate
# assertion at the end.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-cross-build.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-cross.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

# A tree with one plain member and one that compiles C.
make_tree() { # make_tree <dir> <excluded-name>
    d=$1
    mkdir -p "$d/ci" "$d/plain" "$d/native" || exit 2
    printf 'members = ["plain", "native", "a", "b", "c"]\n' > "$d/Cargo.toml"
    printf 'name = "plain"\n' > "$d/plain/Cargo.toml"
    printf 'name = "native"\n[build-dependencies]\ncc = "1"\n' > "$d/native/Cargo.toml"
    for m in a b c; do
        mkdir -p "$d/$m"
        printf 'name = "%s"\n' "$m" > "$d/$m/Cargo.toml"
    done
    printf '#!/bin/sh\nNOT_CROSS_BUILT=%s\n' "$2" > "$d/ci/build-target.sh"
}

# 1. A C-compiling member that is excluded: they agree.
make_tree "$tmp/agree" native
if sh "$guard" --root "$tmp/agree" > /dev/null 2>&1; then
    ok "a C-compiling member that is excluded passes"
else
    bad "a C-compiling member that is excluded passes"
fi

# 2. The same member, not excluded: the guard fails and names it.
make_tree "$tmp/missing" ""
if out=$(sh "$guard" --root "$tmp/missing" 2>&1); then
    out=''
fi
if [ -n "$out" ] && printf '%s' "$out" | grep -q native; then
    ok "a C-compiling member that is not excluded fails, and is named"
else
    bad "a C-compiling member that is not excluded fails, and is named"
fi

# 3. An exclusion naming nothing: a stale name excludes nothing, and
#    hiding that is how a guard stops guarding.
make_tree "$tmp/stale" was-renamed
if out=$(sh "$guard" --root "$tmp/stale" 2>&1); then
    out=''
fi
if [ -n "$out" ] && printf '%s' "$out" | grep -q was-renamed; then
    ok "an exclusion that names no member fails"
else
    bad "an exclusion that names no member fails"
fi

# 4. A tree it cannot read is exit 2, not a pass.
mkdir -p "$tmp/empty"
sh "$guard" --root "$tmp/empty" > /dev/null 2>&1
code=$?
if [ "$code" -eq 2 ]; then
    ok "a tree with no manifest is exit 2"
else
    bad "a tree with no manifest is exit 2"
fi

# 5. And the repository itself agrees.
if sh "$guard" --root "$root" > /dev/null 2>&1; then
    ok "the repository's exclusions and its C-compiling members agree"
else
    bad "the repository's exclusions and its C-compiling members agree"
fi

exit "$rc"
