#!/bin/sh
# Run every non-Rust SDK's own test suite, each in its package's idiom:
# unittest, ExUnit, JUnit through maven, xUnit through `dotnet test`
# (docs/TESTING.md, "The hook protocol and its SDKs").
#
# `rue sdk-conform` judges an SDK from the far end of the wire, which is what
# makes the five comparable; these suites judge what the wire cannot see --
# a secret formatted by accident, a handler over its budget, a line that
# kills the loop -- and need no rue binary at all.
#
# They run where sdk/conform-all.sh runs, for the same reason: on the Ubuntu
# reaper guest and in the pipeline, never in the local gate, whose base
# FreeBSD workstation carries none of these toolchains. And for the same
# reason a missing toolchain is a FAILURE, not a skip: this script is only
# invoked where the toolchains are provisioned.
#
# Selection matches sdk/conform-all.sh: no SDK named runs all four; naming
# some runs those alone, and a named SDK with no toolchain still fails.
#
# The JUnit and xUnit suites resolve their test frameworks from Maven Central
# and NuGet, so those two need the network (or a warm cache) where the
# conformance runs do not. The frameworks are test scope only and reach no
# consumer of either package.
#
# Usage: sh sdk/test-all.sh [python|elixir|java|dotnet ...]
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
usage() {
    echo "usage: test-all.sh [python|elixir|java|dotnet ...]" >&2
    exit 2
}
only=""
while [ $# -gt 0 ]; do
    case $1 in
        python|elixir|java|dotnet) only="$only $1" ;;
        *) usage ;;
    esac
    shift
done

wanted() { # wanted <sdk>: named on the command line, or nothing was named
    [ -z "$only" ] && return 0
    case "$only " in *" $1 "*) return 0 ;; esac
    return 1
}

rc=0
suite() { # suite <sdk> <tool> <command...>: run <command> in sdk/<sdk>
    sdk=$1; shift
    tool=$1; shift
    printf '== %s\n' "$sdk"
    if [ ! -d "$root/sdk/$sdk" ]; then
        echo "test-all: sdk/$sdk is missing from this tree" >&2
        rc=1
        return
    fi
    if ! command -v "$tool" > /dev/null 2>&1; then
        echo "test-all: $sdk needs $tool, which is not on PATH here" >&2
        rc=1
        return
    fi
    if ( cd "$root/sdk/$sdk" && "$@" ); then
        printf 'ok      %s suite passes\n' "$sdk"
    else
        printf 'not ok  %s suite fails\n' "$sdk" >&2
        rc=1
    fi
}

# unittest exits nonzero when it finds no tests (Python 3.12 and later), so
# a discovery that silently matched nothing fails rather than passing.
if wanted python; then
    suite python python3 python3 -m unittest discover -s tests -t .
fi

# Warnings are errors for the library and the tests alike, as they are for
# every other SDK here.
# shellcheck disable=SC2329,SC2317
#   exunit is invoked indirectly, through `suite elixir mix exunit`,
#   which the linter cannot follow: SC2329 in 0.10 and later, SC2317 per
#   line in 0.9 (the note at the top of tools/check.sh). Nothing else is
#   silenced.
exunit() {
    env MIX_ENV=test mix compile --warnings-as-errors && env MIX_ENV=test mix test --warnings-as-errors
}
if wanted elixir; then
    suite elixir mix exunit
fi

if wanted java; then
    suite java mvn mvn -B -ntp test
fi

# The same SDK home as sdk/conform-all.sh: on the guest it lives in a cache.
# DOTNET_ROOT is what lets the test project's apphost find the runtime when
# the SDK is not installed where the apphost looks by default.
if wanted dotnet; then
    if [ -d /tank/cache/dotnet ] && [ -z "${DOTNET_ROOT:-}" ]; then
        DOTNET_ROOT=/tank/cache/dotnet
        DOTNET_CLI_HOME=${DOTNET_CLI_HOME:-/tank/cache/dotnet-home}
        PATH=$DOTNET_ROOT:$PATH
        export DOTNET_ROOT DOTNET_CLI_HOME PATH
    fi
    export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
    suite dotnet dotnet dotnet test tests/RueHook.Tests
fi

if [ "$rc" -eq 0 ]; then
    if [ -z "$only" ]; then
        echo "test-all: every SDK suite passes"
    else
        echo "test-all:$only pass"
    fi
fi
exit "$rc"
