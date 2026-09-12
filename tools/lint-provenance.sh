#!/bin/sh
# The provenance guard (docs/issues/0017, ROADMAP.md section 12).
#
# Two sets of files in this tree are evidence about the project's own past:
# the upgrade vectors (`tenants/_upgrade/<release>/`), which are the tenant
# texts as a release shipped them, and the store fixtures
# (`engine/tests/fixtures/store-<release>/`), which are what that release's
# engine wrote. Phase 5's acceptance line rests on both being what they claim.
#
# Both used to claim it in prose and nothing checked either. A vector edited by
# hand, or copied from a working tree rather than from the tag, would pass
# every suite and turn the upgrade test into a test of today's text against
# today's build -- the thing being checked and the thing doing the checking
# arriving from the same place, which is the failure this project keeps finding
# elsewhere. Naming the TAG is not enough on its own: a tag is a movable ref,
# and "the repository" does not have one state.
#
# So each set carries a PROVENANCE record naming the COMMIT, and this reads it:
#
#   kind = tag         the files must be byte for byte <prefix>/<path> at that
#                      commit. If this clone has the tag, it must still resolve
#                      to that commit -- a moved tag is reported before any
#                      byte comparison, because it explains every mismatch
#                      after it.
#   kind = committed   the files must be byte for byte <dir>/<path> at that
#                      commit, AND that commit must still be the last one to
#                      touch the directory. Store fixtures cannot be
#                      regenerated -- the engine that wrote them is gone -- so
#                      "unchanged since it was recorded" is the strongest
#                      checkable claim, and the record says so in place of
#                      pretending otherwise.
#
# It also requires every directory beside a record to HAVE a section, so a
# vector added without a record fails rather than passing quietly.
#
# This needs the repository's own history, and that is the point rather than an
# inconvenience: the check exists precisely because the working tree cannot be
# its own witness. Every environment that runs the gate carries `.git` (reaper
# syncs it; the pipeline's gate step clones full depth) and git itself, so this
# phase is declared skippable nowhere.
#
# Deliberately dumb: it reads the record as data and asks git for the bytes.
# Tier 3 of the testing methodology.
#
# Usage: lint-provenance.sh [--root DIR]
#
# Exit 0: every file matches its record.  Exit 1: a file differs, a record is
# missing or incomplete, a tag moved, or a fixture has been touched since.
# Exit 2: the guard could not read what it needs -- no git, no work tree, a
# shallow clone -- which is louder than a pass.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
while [ $# -gt 0 ]; do
    case $1 in
        --root) root=$2; shift ;;
        *) echo "usage: lint-provenance.sh [--root DIR]" >&2; exit 2 ;;
    esac
    shift
done

command -v git > /dev/null 2>&1 ||
    { echo "lint-provenance: no git; this guard reads the repository's history" >&2; exit 2; }

git -C "$root" rev-parse --is-inside-work-tree > /dev/null 2>&1 ||
    { echo "lint-provenance: $root is not a git work tree; the records name commits" \
           "that only a repository can supply" >&2; exit 2; }

if [ "$(git -C "$root" rev-parse --is-shallow-repository 2> /dev/null)" = "true" ]; then
    echo "lint-provenance: this is a shallow clone; the recorded commits are older" \
         "than its history and cannot be read. Clone at full depth" >&2
    exit 2
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-prov.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
fail() { echo "lint-provenance: $*" >&2; rc=1; }

# The records this guard knows about. Each is a file whose sections name
# directories BESIDE it.
records='tenants/_upgrade/PROVENANCE engine/tests/fixtures/PROVENANCE'

# Normalize a record to "<section>\t<key>\t<value>" lines, one per setting, and
# report a syntax error rather than reading past it.
parse() { # parse <file> <out>
    awk -v out="$2" '
        /^[ \t]*#/ { next }
        /^[ \t]*$/ { next }
        /^[ \t]*\[[^][]+\][ \t]*$/ {
            line = $0
            sub(/^[ \t]*\[/, "", line)
            sub(/\][ \t]*$/, "", line)
            sec = line
            next
        }
        {
            line = $0
            sub(/^[ \t]+/, "", line)
            sub(/[ \t]+$/, "", line)
            i = index(line, "=")
            if (i == 0) { printf "line %d: not a key = value: %s\n", NR, line > "/dev/stderr"; bad = 1; next }
            if (sec == "") { printf "line %d: a setting before any [section]\n", NR > "/dev/stderr"; bad = 1; next }
            k = substr(line, 1, i - 1); v = substr(line, i + 1)
            sub(/[ \t]+$/, "", k); sub(/^[ \t]+/, "", v)
            printf "%s\t%s\t%s\n", sec, k, v >> out
        }
        END { exit bad ? 1 : 0 }
    ' "$1"
}

val() { # val <normalized> <section> <key>  -- prints the value, empty if unset
    awk -F '\t' -v s="$2" -v k="$3" '$1 == s && $2 == k { print $3; found = 1 }
                                     END { exit !found }' "$1"
}

for record in $records; do
    path=$root/$record
    base=$(dirname -- "$path")
    [ -r "$path" ] || { echo "lint-provenance: cannot read $record" >&2; exit 2; }

    : > "$tmp/kv"
    parse "$path" "$tmp/kv" || { fail "$record: unreadable (see above)"; continue; }

    sections=$(cut -f1 "$tmp/kv" | awk '!seen[$0]++')
    [ -n "$sections" ] || { fail "$record: no sections; the guard read nothing"; continue; }

    # Every directory beside the record must be accounted for. A vector added
    # without a record is the case this catches.
    for d in "$base"/*/; do
        [ -d "$d" ] || continue
        name=$(basename -- "$d")
        printf '%s\n' "$sections" | grep -qx -- "$name" ||
            fail "$record: $name/ has no section; every directory beside the record needs one"
    done

    for sec in $sections; do
        dir=$base/$sec
        [ -d "$dir" ] || { fail "$record: [$sec] names no directory"; continue; }

        kind=$(val "$tmp/kv" "$sec" kind) || kind=''
        commit=$(val "$tmp/kv" "$sec" commit) || commit=''

        case $commit in
            *[!0123456789abcdef]*|'') fail "$record [$sec]: commit is not a full hex object name"; continue ;;
        esac
        [ "${#commit}" -eq 40 ] ||
            { fail "$record [$sec]: commit is ${#commit} characters, not 40; abbreviations are not a record"; continue; }

        git -C "$root" cat-file -e "$commit^{commit}" 2> /dev/null ||
            { fail "$record [$sec]: commit $commit is not in this repository"; continue; }

        # The directory relative to the repository root, for `committed`
        # sections and for the last-touched check.
        dirrel=${dir#"$root"/}

        case $kind in
            tag)
                ref=$(val "$tmp/kv" "$sec" ref) || ref=''
                prefix=$(val "$tmp/kv" "$sec" prefix) || prefix=''
                if [ -z "$ref" ] || [ -z "$prefix" ]; then
                    fail "$record [$sec]: kind = tag needs both ref and prefix"
                    continue
                fi

                # THE DIRECTORY IS A LABEL AND IT MUST MATCH THE REF.
                #
                # Everything else here compares bytes against a commit, which
                # catches a vector whose CONTENT is wrong. It cannot catch a
                # vector that is internally consistent and MISLABELLED: swap
                # two releases' records and each directory still matches the
                # commit its own record names, so the guard passes while
                # `tenants/_upgrade/v0.1.0` holds another release's text and
                # the upgrade test checks the wrong era against today's build.
                #
                # Found by the coop room's rule that a fixture with ONE of
                # something tests fewer rules than it appears to (wren, log
                # 663): the self-test had one vector per tree, so no case in
                # it could tell a vector from the wrong vector.
                if [ "$ref" != "$sec" ]; then
                    fail "$record [$sec]: the directory is named $sec and its record names ref $ref;\
 a vector's directory is its label and a mismatch means one of them is another release's"
                fi

                # A moved tag is reported first: it explains every byte
                # mismatch below it, and a clone without the tag is not a
                # failure -- the commit is the record, the tag is the human's
                # half of it.
                if at=$(git -C "$root" rev-parse -q --verify "refs/tags/$ref^{commit}" 2> /dev/null); then
                    [ "$at" = "$commit" ] ||
                        fail "$record [$sec]: tag $ref now resolves to $at, not the recorded $commit; the tag moved"
                else
                    echo "lint-provenance: $record [$sec]: this clone has no tag $ref;" \
                         "checking against the recorded commit alone"
                fi
                at_prefix=$prefix
                ;;
            committed)
                last=$(git -C "$root" log -1 --format=%H -- "$dirrel" 2> /dev/null) || last=''
                if [ -z "$last" ]; then
                    fail "$record [$sec]: no commit in this history touches $dirrel"
                    continue
                fi
                [ "$last" = "$commit" ] ||
                    fail "$record [$sec]: $dirrel was last touched by $last, not the recorded $commit; \
a fixture that changed is a fixture that no longer proves what it says"

                schema=$(val "$tmp/kv" "$sec" schema) || schema=''
                if [ -n "$schema" ]; then
                    if [ -r "$dir/schema" ]; then
                        have=$(cat -- "$dir/schema")
                        [ "$have" = "$schema" ] ||
                            fail "$record [$sec]: schema says $schema, $sec/schema says $have"
                    else
                        fail "$record [$sec]: schema = $schema but $sec/schema does not exist"
                    fi
                fi
                at_prefix=$dirrel
                ;;
            *)
                fail "$record [$sec]: kind = ${kind:-<unset>} is not one this guard knows (tag, committed)"
                continue
                ;;
        esac

        find "$dir" -type f | sort > "$tmp/files"
        n=$(grep -c . "$tmp/files" 2> /dev/null || echo 0)
        [ "$n" -gt 0 ] ||
            { fail "$record [$sec]: the directory is empty; an empty vector proves nothing"; continue; }

        while IFS= read -r f; do
            rel=${f#"$dir"/}
            blob=$at_prefix/$rel
            if ! git -C "$root" rev-parse -q --verify "$commit:$blob" > /dev/null 2>&1; then
                fail "$record [$sec]: $rel does not exist at $commit as $blob"
                continue
            fi
            if ! git -C "$root" cat-file blob "$commit:$blob" 2> /dev/null | cmp -s - "$f"; then
                fail "$record [$sec]: $rel differs from $blob at $commit"
            fi
        done < "$tmp/files"
    done
done

[ "$rc" -eq 0 ] && echo "lint-provenance: every recorded file matches the commit it names"
exit "$rc"
