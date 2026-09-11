#!/bin/sh
# The issue tracker's guard (docs/issues/README.md; ROADMAP.md section 12).
#
# Bitbucket Cloud retired its issue tracker in August 2026, and the owner
# chose one in the repository (2026-09-11): a file per issue under
# docs/issues/, and an index in its README. This holds the tracker to its
# own rules, so the index a person reads is never stale and no issue is
# closed without saying by what:
#
#   * every issue is docs/issues/NNNN-slug.md, and its first line is
#     `# NNNN: title` with the same number; numbers are unique;
#   * its header names a status (open, in-progress, closed), a kind
#     (defect, feature, question, not-proven), a phase and the date it was
#     opened; a closed issue names the date it was closed, and an issue
#     that is not closed names none. (The commit that closes one says
#     `Closes #NNNN`; it cannot name its own hash in the file it changes.)
#   * the README's index has exactly one row per issue, with its title,
#     kind and status as the issue itself says them;
#   * nothing else lives in the directory.
#
# Usage: lint-issues.sh [--root DIR]
#
# Exit 0: the tracker is in order.  Exit 1: an issue or the index is wrong.
# Exit 2: there is no tracker or no index -- the guard checked nothing.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-issues.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done
dir=$root/docs/issues
readme=$dir/README.md
[ -f "$readme" ] || { echo "lint-issues: no $readme" >&2; exit 2; }

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-lint-issues.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
bad() { echo "lint-issues: $*" >&2; rc=1; }

# The index: rows `| NNNN | title | kind | status |` of the README's table.
sed -n 's/^| \([0-9][0-9][0-9][0-9]\) | \(.*\) | \([a-z-]*\) | \([a-z-]*\) |$/\1	\2	\3	\4/p' "$readme" > "$tmp/index"
if ! grep -q '^| # | Title | Kind | Status |$' "$readme"; then
    echo "lint-issues: $readme has no index table (| # | Title | Kind | Status |)" >&2
    exit 2
fi

: > "$tmp/issues"
for f in "$dir"/*; do
    name=${f##*/}
    [ "$name" = README.md ] && continue
    case $name in
        [0-9][0-9][0-9][0-9]-*.md) ;;
        *) bad "docs/issues/$name is not an issue (NNNN-slug.md) and not the README"; continue ;;
    esac
    num=${name%%-*}
    first=$(sed -n '1p' "$f")
    title=${first#"# $num: "}
    if [ "$title" = "$first" ] || [ -z "$title" ]; then
        bad "docs/issues/$name: the first line must be \`# $num: title\`"
        continue
    fi
    case $title in
        *'|'*) bad "docs/issues/$name: a title with | in it would break the index table" ;;
    esac
    field() { sed -n "s/^- $1: \\(.*\\)$/\\1/p" "$f" | head -1; }
    status=$(field status)
    kind=$(field kind)
    phase=$(field phase)
    opened=$(field opened)
    closed=$(field closed)
    case $status in
        open|in-progress|closed) ;;
        *) bad "docs/issues/$name: status is \`$status\`, not open, in-progress or closed" ;;
    esac
    case $kind in
        defect|feature|question|not-proven) ;;
        *) bad "docs/issues/$name: kind is \`$kind\`, not defect, feature, question or not-proven" ;;
    esac
    [ -n "$phase" ] || bad "docs/issues/$name: no phase (a roadmap phase, or -)"
    case $opened in
        [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]) ;;
        *) bad "docs/issues/$name: opened is \`$opened\`, not a date (YYYY-MM-DD)" ;;
    esac
    if [ "$status" = closed ]; then
        case $closed in
            [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]) ;;
            *) bad "docs/issues/$name: closed, but names no date (- closed: YYYY-MM-DD)" ;;
        esac
    elif [ -n "$closed" ]; then
        bad "docs/issues/$name: names a closing date but is $status"
    fi
    printf '%s\t%s\t%s\t%s\n' "$num" "$title" "$kind" "$status" >> "$tmp/issues"
done

dups=$(cut -f1 "$tmp/issues" | sort | uniq -d)
[ -z "$dups" ] || bad "issue numbers used twice: $dups"

sort "$tmp/issues" > "$tmp/issues.sorted"
sort "$tmp/index" > "$tmp/index.sorted"
if ! cmp -s "$tmp/issues.sorted" "$tmp/index.sorted"; then
    bad "the README's index disagrees with the issues (< an issue as it says itself, > the index row):"
    diff "$tmp/issues.sorted" "$tmp/index.sorted" | grep '^[<>]' | sed 's/^/        /' >&2
fi

if [ "$rc" -eq 0 ]; then
    echo "lint-issues: $(wc -l < "$tmp/issues" | tr -d ' ') issues, the index agrees"
fi
exit "$rc"
