#!/bin/sh
# Self-test of tools/lint-sdk-docs.sh: an SDK with no docs/README.md, an
# example block that drifted from its file, an example naming no file, a
# README with no example, a marker with no block, and a link to nothing
# must each fail; an empty sdk/ must refuse; a well-formed tree and the
# real repository must pass.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-sdk-docs.sh

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-sdk-docs.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

expect() { # expect <status> <label>
    sh "$guard" --root "$tmp/tree" > "$tmp/out" 2>&1
    st=$?
    if [ "$st" -eq "$1" ]; then
        ok "$2"
    else
        bad "$2 (got $st, wanted $1)"; cat "$tmp/out" >&2
    fi
}

# One SDK with a README showing its example, a second page, and a link
# out of the SDK to a document that exists.
reset_tree() {
    rm -rf "$tmp/tree"
    mkdir -p "$tmp/tree/sdk/demo/docs" "$tmp/tree/sdk/demo/examples" "$tmp/tree/docs" || exit 2
    printf 'print("hello")\nprint("hook")\n' > "$tmp/tree/sdk/demo/examples/hello.py"
    printf '# spec\n' > "$tmp/tree/docs/spec.md"
    {
        printf '# demo\n\n<!-- example: examples/hello.py -->\n```python\n'
        cat "$tmp/tree/sdk/demo/examples/hello.py"
        printf '```\n\nSee [the guide](guide.md) and [spec].\n\n[spec]: ../../../docs/spec.md\n'
    } > "$tmp/tree/sdk/demo/docs/README.md"
    printf '# guide\n\nBack to [the README](README.md#demo).\n' > "$tmp/tree/sdk/demo/docs/guide.md"
}

reset_tree
expect 0 "a well-formed tree passes"

reset_tree
printf 'print("changed")\n' >> "$tmp/tree/sdk/demo/examples/hello.py"
expect 1 "an example that drifted from its file is caught"

reset_tree
sed 's|examples/hello.py|examples/gone.py|' "$tmp/tree/sdk/demo/docs/README.md" > "$tmp/r" &&
    mv "$tmp/r" "$tmp/tree/sdk/demo/docs/README.md"
expect 1 "an example naming no file is caught"

reset_tree
mkdir -p "$tmp/tree/sdk/second"
expect 1 "an SDK with no docs/README.md is caught"

reset_tree
printf '# demo\n\nNo code here.\n' > "$tmp/tree/sdk/demo/docs/README.md"
expect 1 "a README with no example held to a file is caught"

reset_tree
printf '\n<!-- example: examples/hello.py -->\nprose, not a fence\n' >> "$tmp/tree/sdk/demo/docs/guide.md"
expect 1 "a marker with no fenced block is caught"

reset_tree
printf '\nSee [nothing](missing.md).\n' >> "$tmp/tree/sdk/demo/docs/guide.md"
expect 1 "a link to nothing is caught"

reset_tree
rm -rf "$tmp/tree/sdk/demo"
expect 2 "an empty sdk/ refuses with exit 2"

if sh "$guard" > "$tmp/out" 2>&1; then
    ok "the repository's SDK docs pass"
else
    bad "the repository's SDK docs pass"; cat "$tmp/out" >&2
fi

exit "$rc"
