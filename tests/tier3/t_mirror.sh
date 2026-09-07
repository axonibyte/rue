#!/bin/sh
# Self-test of ci/mirror.sh: a mirror lands every ref, a second run is a
# no-op, a destination that refuses fails after the declared attempts and
# says so, and a source that does not exist fails the same way.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
script=$root/ci/mirror.sh
command -v bash > /dev/null 2>&1 || { echo "bash is absent" >&2; exit 77; }
command -v git > /dev/null 2>&1 || { echo "git is absent" >&2; exit 77; }

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-mirror.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

# A source with a branch and a tag; an empty bare destination.
git init -q -b main "$tmp/src" && (
    cd "$tmp/src" && git config user.email t@example.invalid && git config user.name t &&
    echo one > f && git add f && git commit -q -m one && git tag -a v1 -m v1
) || exit 2
git init -q --bare "$tmp/dst" || exit 2

if RUE_MIRROR_PAUSE=0 bash "$script" "$tmp/src" "$tmp/dst" > "$tmp/out" 2>&1; then
    ok "mirror succeeds"
else
    bad "mirror succeeds"; cat "$tmp/out" >&2
fi
if [ "$(git -C "$tmp/dst" rev-parse refs/heads/main)" = "$(git -C "$tmp/src" rev-parse refs/heads/main)" ] &&
   [ "$(git -C "$tmp/dst" rev-parse refs/tags/v1)" = "$(git -C "$tmp/src" rev-parse refs/tags/v1)" ]; then
    ok "the branch and the tag landed"
else
    bad "the branch and the tag landed"
fi
if RUE_MIRROR_PAUSE=0 bash "$script" "$tmp/src" "$tmp/dst" > "$tmp/out" 2>&1; then
    ok "a second mirror is a no-op"
else
    bad "a second mirror is a no-op"; cat "$tmp/out" >&2
fi

# A destination that refuses every push (a plain directory, not a repository).
mkdir -p "$tmp/refuse"
if RUE_MIRROR_PAUSE=0 RUE_MIRROR_ATTEMPTS=2 bash "$script" "$tmp/src" "$tmp/refuse" > "$tmp/out" 2>&1; then
    bad "a refusing destination fails"
else
    ok "a refusing destination fails"
fi
if grep -q "attempt 2 of 2 failed" "$tmp/out" && grep -q "giving up after 2 attempts" "$tmp/out"; then
    ok "every attempt is made and reported"
else
    bad "every attempt is made and reported"; cat "$tmp/out" >&2
fi

if RUE_MIRROR_PAUSE=0 RUE_MIRROR_ATTEMPTS=1 bash "$script" "$tmp/nope" "$tmp/dst" > "$tmp/out" 2>&1; then
    bad "a missing source fails"
else
    ok "a missing source fails"
fi

exit "$rc"
