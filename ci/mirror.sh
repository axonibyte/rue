#!/usr/bin/env bash
# ci/mirror.sh  mirror one git repository into another, retrying a lost race.
# Usage: bash ci/mirror.sh <source-url> <destination-url>
#
# A push and a tag push start two pipelines whose mirror steps run at the
# same time; `git push --mirror` from the one that clones first loses the
# ref lock to the one that pushes first ("cannot lock ref ... is at X but
# expected Y") although the destination then holds the newer state. A
# mirror is idempotent, so the loser re-clones and pushes again; only a
# push that fails every attempt is a failure. RUE_MIRROR_ATTEMPTS and
# RUE_MIRROR_PAUSE (seconds) are for the self-test.
set -euo pipefail

src="${1:?usage: mirror.sh <source-url> <destination-url>}"
dst="${2:?usage: mirror.sh <source-url> <destination-url>}"
attempts="${RUE_MIRROR_ATTEMPTS:-3}"
pause="${RUE_MIRROR_PAUSE:-15}"

work=$(mktemp -d "${TMPDIR:-/tmp}/rue-mirror.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM

for attempt in $(seq 1 "$attempts"); do
    rm -rf "$work/mirror.git"
    if git clone --quiet --mirror "$src" "$work/mirror.git" &&
        git -C "$work/mirror.git" push --mirror "$dst"; then
        echo "mirror: $src -> $dst (attempt $attempt)"
        exit 0
    fi
    echo "mirror: attempt $attempt of $attempts failed" >&2
    if [[ "$attempt" -lt "$attempts" ]]; then
        sleep "$pause"
    fi
done
echo "mirror: giving up after $attempts attempts" >&2
exit 1
