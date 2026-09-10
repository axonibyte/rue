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

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-conform-all.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

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

# Java is compiled with javac and not with maven: `mvn compile` resolves
# its plugins from Maven Central, and a conformance run should not need the
# network. The POM is the artifact's packaging story and the pipeline's
# maven step is what proves it builds; what is judged here is the code.
if [ -d "$root/sdk/java" ]; then
    if command -v javac > /dev/null 2>&1; then
        rm -rf "$root/sdk/java/build"
        mkdir -p "$root/sdk/java/build"
        # The source list goes through javac's @argfile rather than through
        # the shell, so there is no word splitting to reason about and no
        # lint to suppress.
        sources=$tmp/java-sources
        find "$root/sdk/java/src/main/java" -name '*.java' > "$sources"
        if ! ( cd "$root/sdk/java" && javac -d build "@$sources" ); then
            echo "conform-all: sdk/java does not compile" >&2
            rc=1
        fi
    fi
    run java java "java -cp $root/sdk/java/build dev.rue.hook.example.ConformanceHook conform"
fi

if [ "$rc" -eq 0 ]; then
    echo "conform-all: every SDK conforms"
fi
exit "$rc"
