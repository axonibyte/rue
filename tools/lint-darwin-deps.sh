#!/bin/sh
# The darwin dependency guard (ROADMAP.md section 12).
#
# The two apple-darwin targets are cross-linked from Linux with zig and no
# macOS SDK, which holds exactly as long as nothing in the darwin dependency
# graph links an Apple framework. This lists that graph with `cargo tree`
# for both darwin triples (no darwin toolchain is needed: cargo resolves
# from Cargo.lock and rustc prints the target's cfg) and fails on any crate
# named in tools/darwin-denylist.txt. Tier 3 of the testing methodology:
# source as data, deliberately dumb.
#
# Usage: lint-darwin-deps.sh [--root DIR] [--denylist FILE] [--from FILE]
#   --from FILE   a saved listing, one crate name per line, instead of
#                 running cargo; the self-test uses it.
#
# Exit 0: clean.  Exit 1: a denied crate is in the graph (printed).
# Exit 2: the guard could not check (empty denylist, unreadable input,
# cargo tree failed).  Exit 77: cargo is absent and no --from was given.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
list=''
from=''

while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        --denylist) list=$2; shift ;;
        --from) from=$2; shift ;;
        *) echo "usage: lint-darwin-deps.sh [--root DIR] [--denylist FILE] [--from FILE]" >&2; exit 2 ;;
    esac
    shift
done
[ -n "$list" ] || list=$root/tools/darwin-denylist.txt

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-darwin.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

if [ ! -r "$list" ]; then
    echo "lint-darwin-deps: cannot read denylist $list" >&2
    exit 2
fi
grep -v -e '^#' -e '^[[:space:]]*$' "$list" > "$tmp/words"
if [ ! -s "$tmp/words" ]; then
    echo "lint-darwin-deps: denylist $list has no crates; the guard checked nothing" >&2
    exit 2
fi

if [ -n "$from" ]; then
    if [ ! -r "$from" ]; then
        echo "lint-darwin-deps: cannot read listing $from" >&2
        exit 2
    fi
    cp "$from" "$tmp/graph"
else
    if ! command -v cargo > /dev/null 2>&1; then
        echo "lint-darwin-deps: cargo is absent; nothing checked" >&2
        exit 77
    fi
    : > "$tmp/graph"
    for triple in aarch64-apple-darwin x86_64-apple-darwin; do
        if ! ( cd "$root" && cargo tree --workspace --locked --target "$triple" -e normal --prefix none ) >> "$tmp/graph" 2> "$tmp/err"; then
            echo "lint-darwin-deps: cargo tree failed for $triple:" >&2
            cat "$tmp/err" >&2
            exit 2
        fi
    done
fi
awk '{ print $1 }' "$tmp/graph" | sort -u > "$tmp/crates"
if [ ! -s "$tmp/crates" ]; then
    echo "lint-darwin-deps: the dependency graph is empty; the guard checked nothing" >&2
    exit 2
fi

grep -F -x -f "$tmp/words" "$tmp/crates" > "$tmp/hits"
if [ -s "$tmp/hits" ]; then
    echo "lint-darwin-deps: FAIL -- a crate that links an Apple framework is in the darwin dependency graph:" >&2
    sed 's/^/  /' "$tmp/hits" >&2
    exit 1
fi
echo "lint-darwin-deps: clean ($(wc -l < "$tmp/crates" | tr -d ' ') crates, none denied)"
exit 0
