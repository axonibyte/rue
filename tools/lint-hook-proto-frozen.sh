#!/bin/sh
# The hook protocol is frozen at v1 (docs/ROADMAP.md, Phase 4).
#
# docs/hook-protocol-v1.json is a golden, generated from rue-hook-proto's
# tables by rue-goldens and compared byte for byte by the suite. That alone
# freezes nothing: change an op, regenerate the goldens, and v1 now means
# something it did not mean yesterday while every test is green. So this
# guard pins each released version's DIGEST here, where changing it is a
# visible edit to a line that says what it is, and refuses any document
# whose bytes have moved.
#
# The only way past both halves is the one that is meant to exist: bump
# HOOK_PROTOCOL, which moves the generated document to -v2.json and leaves
# v1's bytes untouched for anybody still speaking v1. Then add v2's digest
# below, beside v1's, when v2 is released.
#
# Usage: lint-hook-proto-frozen.sh [--root DIR]
#
# Exit 0: every released version is intact and the current one has a
# document.  Exit 1: a violation (printed).  Exit 2: this guard could not
# read what it checks -- which is worse than a failure, since it would
# otherwise report a freeze it never verified.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-hook-proto-frozen.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done

# version  sha256 of docs/hook-protocol-v<version>.json
RELEASED='
1 c79946e55a921135f7ae0573d4623c2d3048c0e1b567fae777858291e8b16c6b
'

digest() {
    if command -v sha256 > /dev/null 2>&1; then
        sha256 -q "$1"
    elif command -v sha256sum > /dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    elif command -v shasum > /dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d' ' -f1
    else
        # Every host the gate runs on has one of the three. A host with none
        # is not one to skip on quietly: the freeze would go unchecked.
        echo "lint-hook-proto-frozen: no sha256, sha256sum or shasum on PATH" >&2
        exit 2
    fi
}

src=$root/hook-proto/src/op.rs
[ -f "$src" ] || { echo "lint-hook-proto-frozen: $src not found" >&2; exit 2; }
current=$(sed -n 's/^pub const HOOK_PROTOCOL: u32 = \([0-9][0-9]*\);.*/\1/p' "$src")
[ -n "$current" ] || { echo "lint-hook-proto-frozen: no HOOK_PROTOCOL constant in $src" >&2; exit 2; }

rc=0
checked=0
while read -r version want; do
    [ -n "$version" ] || continue
    doc=$root/docs/hook-protocol-v$version.json
    if [ ! -f "$doc" ]; then
        echo "lint-hook-proto-frozen: $doc is missing; a released protocol version keeps its document for as long as anything may speak it" >&2
        rc=1
        continue
    fi
    got=$(digest "$doc")
    checked=$((checked + 1))
    if [ "$got" != "$want" ]; then
        echo "lint-hook-proto-frozen: $doc has changed since v$version was released" >&2
        echo "  pinned:  $want" >&2
        echo "  now:     $got" >&2
        echo "  hook protocol v$version is frozen. A change to an op, a field or a kind is a" >&2
        echo "  new protocol version: bump HOOK_PROTOCOL in hook-proto/src/op.rs, regenerate" >&2
        echo "  (RUE_UPDATE_GOLDENS=1 cargo run -p rue-tenants --bin rue-goldens), restore" >&2
        echo "  this file from git, and say in docs/ROADMAP.md what the new version changed." >&2
        rc=1
    fi
done <<LIST
$RELEASED
LIST

[ "$checked" -gt 0 ] || [ "$rc" -ne 0 ] || { echo "lint-hook-proto-frozen: no released version was checked" >&2; exit 2; }

if [ ! -f "$root/docs/hook-protocol-v$current.json" ]; then
    echo "lint-hook-proto-frozen: HOOK_PROTOCOL is $current and docs/hook-protocol-v$current.json does not exist; regenerate the goldens" >&2
    rc=1
fi

[ "$rc" -eq 0 ] && echo "lint-hook-proto-frozen: v$current current; $checked released version(s) intact"
exit "$rc"
