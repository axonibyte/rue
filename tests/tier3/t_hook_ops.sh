#!/bin/sh
# Self-test of the hook-protocol guard: it must fail when the document and
# `OPS` disagree in either direction, and refuse to run against a source it
# could read nothing from.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-hook-ops.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-hook-ops.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

reset_tree() {
    rm -rf "$tmp/tree"
    mkdir -p "$tmp/tree/docs" "$tmp/tree/hook-proto/src" \
             "$tmp/tree/sdk/python/rue_hook" || exit 2
    cp "$root/docs/hook-protocol.md" "$tmp/tree/docs/" || exit 2
    cp "$root/hook-proto/src/op.rs" "$tmp/tree/hook-proto/src/" || exit 2
    cp "$root/sdk/python/rue_hook/proto.py" "$tmp/tree/sdk/python/rue_hook/" || exit 2
}

expect() { # expect <status> <label>
    sh "$guard" --root "$tmp/tree" > "$tmp/out" 2>&1
    st=$?
    if [ "$st" -eq "$1" ]; then ok "$2"; else bad "$2 (got $st)"; cat "$tmp/out" >&2; fi
}

# 1. The real pair agrees.
reset_tree
expect 0 "the repository's document and OPS agree"

# 2. An op documented and not implemented.
reset_tree
# The row's backticks are markdown, so they are built rather than written
# inside a quoted string where they would read as a command substitution.
bt=$(printf '\140')
printf '| %snotify%s | %spage%s | %slevel%s | |\n' \
    "$bt" "$bt" "$bt" "$bt" "$bt" "$bt" >> "$tmp/tree/docs/hook-protocol.md"
expect 1 "an op documented and absent from OPS is caught"

# 3. An op implemented and undocumented: add a row to OPS.
reset_tree
awk '
    /^pub const OPS/ {
        print
        print "    Op {"
        print "        kind: \"notify\","
        print "        op: \"page\","
        print "        request: &[],"
        print "        required_reply: &[],"
        print "        optional_reply: &[],"
        print "        secret: None,"
        print "        optional: false,"
        print "    },"
        next
    }
    { print }
' "$root/hook-proto/src/op.rs" > "$tmp/tree/hook-proto/src/op.rs"
grep -q '"page"' "$tmp/tree/hook-proto/src/op.rs" || bad "sample OPS row added"
expect 1 "an op in OPS and undocumented is caught"

# 4. A source nothing can be read from refuses with exit 2, rather than
#    passing because it compared two empty lists.
reset_tree
: > "$tmp/tree/hook-proto/src/op.rs"
expect 2 "an unreadable OPS refuses with exit 2"

reset_tree
: > "$tmp/tree/docs/hook-protocol.md"
expect 2 "an unreadable document refuses with exit 2"

# 5. An SDK's transcription drifted from OPS, in both directions. This is
#    the leg that rots quietly: an SDK's own tests pass against its own
#    idea of the protocol, so nothing else would notice.
reset_tree
printf '    Op("notify", "page"),\n' >> "$tmp/tree/sdk/python/rue_hook/proto.py"
expect 1 "an op in an SDK's table and absent from OPS is caught"

reset_tree
grep -v '"host_lock"' "$root/sdk/python/rue_hook/proto.py" \
    > "$tmp/tree/sdk/python/rue_hook/proto.py"
expect 1 "an op in OPS and absent from an SDK's table is caught"

# 6. A row written over several lines counts the same as one on a single
#    line: how a table is formatted is not the guard's business.
reset_tree
sed 's/Op("probe", "observe"/Op(\n        "probe",\n        "observe"/' \
    "$root/sdk/python/rue_hook/proto.py" > "$tmp/tree/sdk/python/rue_hook/proto.py"
expect 0 "a row spread over several lines is still read"

# 7. An SDK that is present with no table at all refuses with exit 2,
#    rather than passing because there was nothing to compare. A skipped
#    leg that reports success is the failure every guard here exists to
#    avoid.
reset_tree
rm -f "$tmp/tree/sdk/python/rue_hook/proto.py"
expect 2 "an SDK present with no table refuses with exit 2"

exit "$rc"
