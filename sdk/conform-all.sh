#!/bin/sh
# Judge every SDK that is not Rust against `rue sdk-conform`
# (docs/sdk-conformance.md).
#
# The Rust SDK and the shim are workspace crates, so their conformance runs
# are ordinary `cargo test` targets and the gate covers them everywhere.
# The rest need a toolchain the FreeBSD workstation does not carry, so they
# run here: on the reaper guests and in the pipeline, and nowhere else
# (docs/TESTING.md, "The hook protocol and its SDKs").
#
# A missing interpreter is a FAILURE, not a skip. This script is only ever
# invoked where the toolchains are provisioned, so "not installed" means
# the provisioning is wrong and saying so is the whole point; a skip here
# would report success for work nobody did.
#
# Usage: sh sdk/conform-all.sh [--rue PATH]
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
rue=""
while [ $# -gt 0 ]; do
    case $1 in
        --rue) rue=$2; shift ;;
        *) echo "usage: conform-all.sh [--rue PATH]" >&2; exit 2 ;;
    esac
    shift
done

if [ -z "$rue" ]; then
    for candidate in \
        "${CARGO_TARGET_DIR:-$root/target}/release/rue" \
        "$root/target/release/rue" \
        "${CARGO_TARGET_DIR:-$root/target}/debug/rue"
    do
        [ -x "$candidate" ] && { rue=$candidate; break; }
    done
fi
[ -n "$rue" ] && [ -x "$rue" ] || {
    echo "conform-all: no rue binary; build one or pass --rue PATH" >&2
    exit 2
}

rc=0
run() { # run <sdk> <interpreter> <command...>
    sdk=$1; shift
    tool=$1; shift
    printf '== %s\n' "$sdk"
    if ! command -v "$tool" > /dev/null 2>&1; then
        echo "conform-all: $sdk needs $tool, which is not on PATH here" >&2
        rc=1
        return
    fi
    if "$rue" sdk-conform --name conform "$*"; then
        printf 'ok      %s conforms\n' "$sdk"
    else
        printf 'not ok  %s does not conform\n' "$sdk" >&2
        rc=1
    fi
}

run python python3 "python3 $root/sdk/python/examples/conformance_hook.py conform"

# Elixir is compiled first: `mix compile` is idempotent and cheap once the
# build directory exists, and a hook that has to compile itself on its
# first request would be Silent while it did.
if [ -d "$root/sdk/elixir" ]; then
    if command -v mix > /dev/null 2>&1; then
        ( cd "$root/sdk/elixir" && MIX_ENV=dev mix compile > /dev/null ) || {
            echo "conform-all: sdk/elixir does not compile" >&2
            rc=1
        }
    fi
    run elixir elixir \
        "elixir -pa $root/sdk/elixir/_build/dev/lib/rue_hook/ebin \
         $root/sdk/elixir/examples/conformance_hook.exs conform"
fi

if [ "$rc" -eq 0 ]; then
    echo "conform-all: every SDK conforms"
fi
exit "$rc"
