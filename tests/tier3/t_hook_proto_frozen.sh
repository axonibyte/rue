#!/bin/sh
# Self-test of the hook-protocol freeze: every way of changing v1 in place
# must fail, the one legitimate way forward -- a new version beside it --
# must pass, and a source it cannot read must refuse rather than report a
# freeze it never checked.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-hook-proto-frozen.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-frozen.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

reset_tree() {
    rm -rf "$tmp/tree"
    mkdir -p "$tmp/tree/docs" "$tmp/tree/hook-proto/src" "$tmp/tree/tools" || exit 2
    cp "$root/docs/hook-protocol-v1.json" "$tmp/tree/docs/" || exit 2
    cp "$root/hook-proto/src/op.rs" "$tmp/tree/hook-proto/src/" || exit 2
    # The guard resolves its root from its own location unless told, and
    # this test always tells it; the copy is only so a mistake in that
    # resolution fails here rather than reading the real tree.
    cp "$guard" "$tmp/tree/tools/" || exit 2
}

expect() { # expect <status> <label>
    sh "$guard" --root "$tmp/tree" > "$tmp/out" 2>&1
    st=$?
    if [ "$st" -eq "$1" ]; then ok "$2"; else bad "$2 (got $st)"; cat "$tmp/out" >&2; fi
}

bump() { # bump <version>: set HOOK_PROTOCOL in the copy
    sed "s/^pub const HOOK_PROTOCOL: u32 = [0-9][0-9]*;/pub const HOOK_PROTOCOL: u32 = $1;/" \
        "$tmp/tree/hook-proto/src/op.rs" > "$tmp/op.rs" && mv "$tmp/op.rs" "$tmp/tree/hook-proto/src/op.rs"
}

# 1. The repository as it stands.
reset_tree
expect 0 "the released v1 document is intact"

# 2. THE CASE THE FREEZE EXISTS FOR: an op changed and the golden regenerated
#    over v1. Every test would be green -- the table and the document agree --
#    and v1 would mean something new. Simulated by editing one field in place.
reset_tree
sed 's/"entry"/"entries"/' "$tmp/tree/docs/hook-protocol-v1.json" > "$tmp/v1" \
    && mv "$tmp/v1" "$tmp/tree/docs/hook-protocol-v1.json"
expect 1 "a v1 document regenerated after an op changed is refused"

# 3. Even a change that alters nothing a parser would notice: the bytes are
#    the contract, the same rule every golden here is held to.
reset_tree
printf ' ' >> "$tmp/tree/docs/hook-protocol-v1.json"
expect 1 "a v1 document with one byte appended is refused"

# 4. A released version's document deleted: whoever still speaks v1 has lost
#    the only statement of what it is.
reset_tree
rm "$tmp/tree/docs/hook-protocol-v1.json"
expect 1 "a missing v1 document is refused"

# 5. Bumped to v2 but never regenerated: the tree claims a version it has no
#    document for.
reset_tree
bump 2
expect 1 "HOOK_PROTOCOL 2 with no v2 document is refused"

# 6. THE LEGITIMATE PATH: v2 declared, its document present, v1 untouched.
#    A freeze that blocked this too would not be a freeze, it would be a wall.
reset_tree
bump 2
printf '{\n  "protocol": 2\n}\n' > "$tmp/tree/docs/hook-protocol-v2.json"
expect 0 "a new version beside an intact v1 passes"

# 7. And v1 edited while moving to v2 is still refused: moving on is not a
#    licence to rewrite what came before.
reset_tree
bump 2
printf '{\n  "protocol": 2\n}\n' > "$tmp/tree/docs/hook-protocol-v2.json"
printf ' ' >> "$tmp/tree/docs/hook-protocol-v1.json"
expect 1 "v1 edited during a move to v2 is refused"

# 8. A source with no HOOK_PROTOCOL constant: the guard cannot know what is
#    current, and must say so instead of passing.
reset_tree
sed '/^pub const HOOK_PROTOCOL/d' "$tmp/tree/hook-proto/src/op.rs" > "$tmp/op.rs" \
    && mv "$tmp/op.rs" "$tmp/tree/hook-proto/src/op.rs"
expect 2 "no HOOK_PROTOCOL constant is a refusal, not a pass"

exit "$rc"
