#!/bin/sh
# The cross-build guard (ROADMAP.md section 12).
#
# The darwin targets are cross-linked from Linux with zig and no macOS SDK,
# and nothing in that build has a C compiler that takes `-arch` or
# `-mmacosx-version-min`. A workspace member whose build script compiles C
# therefore cannot be cross-built, and must be named in `ci/build-target.sh`'s
# `NOT_CROSS_BUILT` -- or the pipeline finds out for us, four steps and
# twenty minutes after the gate said yes, which is how `tree-sitter-rue`
# was found.
#
# So: every member with a `cc` build dependency is excluded, and every name
# excluded is a real member. It does not decide what *should* be shipped --
# that is the author's -- only that the two lists agree.
#
# Deliberately dumb: it reads Cargo.toml and the build script as data. Tier
# 3 of the testing methodology.
#
# Usage: lint-cross-build.sh [--root DIR]
#
# Exit 0: they agree.  Exit 1: a C-compiling member is not excluded, or an
# excluded name is not a member.  Exit 2: the guard could not read what it
# needs, which is louder than a pass.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-cross-build.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done

script=$root/ci/build-target.sh
manifest=$root/Cargo.toml
[ -r "$script" ] || { echo "lint-cross-build: cannot read $script" >&2; exit 2; }
[ -r "$manifest" ] || { echo "lint-cross-build: cannot read $manifest" >&2; exit 2; }

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-cross.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

# The workspace's members, one directory per line.
sed -n 's/^members = \[\(.*\)\]$/\1/p' "$manifest" |
    tr ',' '\n' | tr -d ' "' | grep -v '^$' > "$tmp/members"
n=$(grep -c . "$tmp/members" 2> /dev/null || echo 0)
[ "$n" -ge 5 ] || { echo "lint-cross-build: only $n workspace members found; the guard read nothing" >&2; exit 2; }

# What the build script excludes from every cross-target build.
sed -n 's/^NOT_CROSS_BUILT=\(.*\)$/\1/p' "$script" | tr -d '"' | grep -v '^$' > "$tmp/excluded"

rc=0

# Every member whose build dependencies name `cc` compiles C.
while read -r dir; do
    toml=$root/$dir/Cargo.toml
    [ -r "$toml" ] || continue
    # The package name, and whether the manifest has a cc build dependency.
    name=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$toml" | head -1)
    [ -n "$name" ] || continue
    if awk '/^\[build-dependencies\]/ {on=1; next} /^\[/ {on=0} on' "$toml" |
        grep -q '^cc[ =]'; then
        if ! grep -qx "$name" "$tmp/excluded"; then
            echo "lint-cross-build: $name compiles C and is not in NOT_CROSS_BUILT ($dir)" >&2
            rc=1
        fi
    fi
done < "$tmp/members"

# And nothing is excluded that is not a member: a stale name excludes
# nothing and hides the fact that it excludes nothing.
while read -r name; do
    found=0
    while read -r dir; do
        toml=$root/$dir/Cargo.toml
        [ -r "$toml" ] || continue
        if [ "$(sed -n 's/^name = "\(.*\)"$/\1/p' "$toml" | head -1)" = "$name" ]; then
            found=1
            break
        fi
    done < "$tmp/members"
    if [ "$found" -eq 0 ]; then
        echo "lint-cross-build: NOT_CROSS_BUILT names $name, which is not a workspace member" >&2
        rc=1
    fi
done < "$tmp/excluded"

[ "$rc" -eq 0 ] && echo "lint-cross-build: the cross-build exclusions and the C-compiling members agree"
exit "$rc"
