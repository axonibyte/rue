#!/bin/sh
# The SDK user-docs guard (ROADMAP.md section 10, tier 3).
#
# Each SDK under sdk/ carries user docs in its own docs/ directory, and a
# page of code that no longer matches the SDK is worse than no page. So:
#
#   * every SDK directory has docs/README.md, and that README shows at least
#     one example the guard can hold to a file;
#   * every block introduced by `<!-- example: PATH -->` is byte for byte
#     the file PATH (relative to the SDK's directory), which that SDK's own
#     suite compiles and tests -- the page cannot drift from the code
#     without this failing;
#   * every relative link in the docs resolves to something that exists.
#
# Usage: lint-sdk-docs.sh [--root DIR]
#
# Exit 0: every SDK's docs are in order.  Exit 1: a README, an example or a
# link is missing or wrong.  Exit 2: no SDK directory at all -- the guard
# checked nothing.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-sdk-docs.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-lint-sdk-docs.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
bad() { echo "lint-sdk-docs: $*" >&2; rc=1; }

sdks=0
for dir in "$root"/sdk/*/; do
    [ -d "$dir" ] || continue
    sdks=$((sdks + 1))
    sdk=${dir%/}
    sdk=${sdk##*/}
    docs=$dir/docs
    if [ ! -f "$docs/README.md" ]; then
        bad "sdk/$sdk has no docs/README.md"
        continue
    fi
    examples=0
    for page in "$docs"/*.md; do
        rel=sdk/$sdk/docs/${page##*/}
        # Each example block to its own file, and a list of what each one
        # claims to be: "N<TAB>PATH". A marker not followed by a fence, or a
        # fence never closed, is reported by name.
        rm -f "$tmp"/block.* "$tmp/list"
        awk -v out="$tmp" -v page="$rel" '
            BEGIN { n = 0; state = 0 }
            state == 2 {
                if ($0 ~ /^```[ \t]*$/) { state = 0; close(file); next }
                print > file
                next
            }
            state == 1 {
                if ($0 !~ /^```/) {
                    printf "%s:%d: an example marker must be followed by a fenced block\n", page, NR > "/dev/stderr"
                    bad = 1; state = 0; next
                }
                state = 2; next
            }
            /^<!-- example: [^ ]+ -->[ \t]*$/ {
                n++
                path = $0
                sub(/^<!-- example: /, "", path)
                sub(/ -->[ \t]*$/, "", path)
                file = out "/block." n
                printf "" > file
                printf "%d\t%s\n", n, path >> (out "/list")
                state = 1
                next
            }
            END {
                if (state != 0) {
                    printf "%s: an example block is never closed\n", page > "/dev/stderr"
                    bad = 1
                }
                exit bad
            }
        ' "$page" || rc=1
        if [ -f "$tmp/list" ]; then
            while IFS="$(printf '\t')" read -r n path; do
                examples=$((examples + 1))
                if [ ! -f "$dir/$path" ]; then
                    bad "$rel: the example $path does not exist in sdk/$sdk"
                elif ! cmp -s "$tmp/block.$n" "$dir/$path"; then
                    bad "$rel: the example block differs from sdk/$sdk/$path (make the page show the file)"
                fi
            done < "$tmp/list"
        fi
        # Relative links, inline and by reference: each must resolve from
        # the page's directory. URLs and in-page anchors are not files.
        { grep -o '](\([^)]*\))' "$page" | sed 's/^](\(.*\))$/\1/'
          sed -n 's/^\[[^]]*\]: *\([^ ]*\).*$/\1/p' "$page"; } |
        while IFS= read -r target; do
            case $target in
                ''|'#'*|http://*|https://*|mailto:*) continue ;;
            esac
            file=${target%%#*}
            [ -e "$docs/$file" ] || echo "$rel: the link $target does not resolve"
        done > "$tmp/links"
        if [ -s "$tmp/links" ]; then
            sed 's/^/lint-sdk-docs: /' "$tmp/links" >&2
            rc=1
        fi
    done
    case $(sed -n '/^<!-- example: [^ ]* -->/p' "$docs/README.md" | head -1) in
        '') bad "sdk/$sdk/docs/README.md shows no example held to a file" ;;
    esac
    [ "$examples" -gt 0 ] && echo "ok      sdk/$sdk: $examples example(s) match their files"
done

if [ "$sdks" -eq 0 ]; then
    echo "lint-sdk-docs: no SDK directories under $root/sdk; the guard checked nothing" >&2
    exit 2
fi
exit "$rc"
