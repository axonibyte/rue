#!/bin/sh
# ci/sdk-conform.sh  judge one SDK against the rue this pipeline built.
# Usage: sh ci/sdk-conform.sh <python|elixir|java|dotnet>
#
# Each SDK of ROADMAP.md 7.11 that is not Rust runs in the image that carries
# its toolchain, and none of those images carries cargo. So the step does not
# build rue: it takes the Linux x86_64 binary buildLinuxAmd64 left in dist/,
# the artifact doDeploy publishes, and hands it to sdk/conform-all.sh, the
# same entry point the Ubuntu reaper guest runs for all four at once.
set -eu

sdk=${1:?usage: sdk-conform.sh <python|elixir|java|dotnet>}
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)

# Exactly one: none means the build step's artifact never arrived, and more
# than one means dist/ holds two versions and nothing says which was built.
set -- "$root"/dist/rue-v*-x86_64-unknown-linux-gnu
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
    echo "sdk-conform: expected one dist/rue-v*-x86_64-unknown-linux-gnu, found: $*" >&2
    exit 1
fi
rue=$1
chmod +x "$rue"
"$rue" --version

exec sh "$root/sdk/conform-all.sh" --rue "$rue" "$sdk"
