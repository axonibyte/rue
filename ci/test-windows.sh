#!/usr/bin/env bash
# ci/test-windows.sh  run rue's whole test suite built for Windows, under wine.
# Usage: bash ci/test-windows.sh
#
# The Windows proof this phase has, wherever a Debian-family host with cargo
# is available: the Bitbucket pipeline (rust:1.97-trixie) and the Ubuntu reaper
# guest (the GHC image plus rustup). It proves the crates' logic and the CLI's
# bytes on x86_64-pc-windows-gnu; what wine cannot exercise -- services, named
# pipes, the Task Scheduler, ACLs -- is Phase 3's to test on a real machine.
# Per-OS knowledge lives in exactly three places (ROADMAP.md section 4.5); this
# is part of the ci/ one.
set -euo pipefail

root=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

apt_install() {
    apt-get -qq update
    apt-get -qq install -y --no-install-recommends "$@"
}

# The cross linker and wine, installed if the host lacks them (a Debian-family
# host is assumed for the install; a host that already has both needs no apt).
# Debian's wine64 package ships only /usr/lib/wine/wine64; the wine package is
# what puts a loader on PATH. Whichever is present is the runner.
if ! command -v x86_64-w64-mingw32-gcc > /dev/null 2>&1; then
    apt_install gcc-mingw-w64-x86-64
fi
find_wine() {
    for candidate in wine wine64 /usr/lib/wine/wine64; do
        if command -v "$candidate" > /dev/null 2>&1; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}
if ! runner=$(find_wine); then
    apt_install wine
    runner=$(find_wine) || { echo "wine installed but no loader found on PATH or in /usr/lib/wine" >&2; exit 1; }
fi
echo "wine runner: $runner ($("$runner" --version 2> /dev/null || echo 'version unknown'))"

rustup target add x86_64-pc-windows-gnu
rustup component add clippy

export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER="$runner"
# No display, no fixme chatter but errors kept (a binary that fails to load
# says which DLL), a prefix of our own, and the repository root stated so the
# golden tests need no ancestor walk under wine's paths.
export WINEDEBUG=fixme-all
export WINEPREFIX="${WINEPREFIX:-${CARGO_TARGET_DIR:-$root/target}/wine}"
export RUE_REPO_ROOT="$root"
mkdir -p "$WINEPREFIX"
# Debian's wine 10 (trixie, 10.0~repack-6) aborts with "free(): invalid
# pointer" at startup when TMPDIR is set and /run/user/<uid> is not writable
# (Debian bug #1110936, in its temporary-directory patch). Containers have no
# /run/user, and the reaper run sets TMPDIR for the gate; neither belongs to
# wine, so TMPDIR is cleared here and the directory the patch wants is made.
unset TMPDIR
mkdir -p "/run/user/$(id -u)" 2> /dev/null || true

# Prove the loader before the suite: a fresh prefix is created here, once,
# where its output is legible, rather than inside cargo's first test run.
"$runner" wineboot --init
"$runner" cmd /c 'echo wine loader ok'
# One wineserver for the whole suite. Left to itself the server exits a few
# seconds after its last client goes, and cargo starts the next test binary
# inside that window; a client that connects to a server on its way out
# gets "wine client error: recvmsg: Connection reset by peer" and its test
# "exited abnormally" with nothing else wrong. -p keeps the server until
# the -k at the end.
wineserver -w 2> /dev/null || true
wineserver -p

# Clippy on the Windows target before the suite (Phase 1 acceptance: clippy
# clean on every target); lint needs the target's std, not wine.
cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-gnu -- -D warnings

# The windows-gnu target links the C runtime statically (.cargo/config.toml),
# so the test binaries need no mingw DLLs under wine.
# rue-e2e is the tier 5 harness; it runs on a reaper guest only (tools/check.sh).
status=0
cargo test --workspace --exclude rue-e2e --release --locked --target x86_64-pc-windows-gnu || status=$?
# The suite's status is the script's; the server's shutdown is not.
wineserver -k || echo "wineserver: nothing left to stop"
exit "$status"
