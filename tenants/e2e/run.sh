#!/bin/sh
# The tier 5 and 6 run on a disposable reaper guest: provision, then the
# harness's tests against the guest itself. Never a gate phase; the guest's
# [run] in .reaper.toml is the only caller (docs/TESTING.md, "Under reaper").
#
# Refuses anywhere that is not a reaper guest unless RUE_E2E_DISPOSABLE=1:
# provisioning rewrites sshd's drop-ins, the firewall and the loopback
# configuration of the machine it runs on.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
cd "$root" || exit 2

if [ -z "${REAPER_WORK:-}" ] && [ "${RUE_E2E_DISPOSABLE:-}" != 1 ]; then
    echo "run.sh: not a reaper guest (REAPER_WORK unset) and RUE_E2E_DISPOSABLE is not 1; refusing to provision this machine" >&2
    exit 2
fi
command -v cargo > /dev/null 2>&1 || { echo "run.sh: cargo is not on PATH; the guest's build installs a pinned toolchain into its cache" >&2; exit 2; }

echo "== provision"
sh tenants/e2e/provision.sh apply || exit 1
sh tenants/e2e/provision.sh check || exit 1

# reaper syncs the tree with the workstation's modification times, and the
# guest keeps its cargo cache between runs. A source whose mtime is older
# than the cached artifact is not rebuilt, so a change can be invisible
# here run after run. Touching the tree with the guest's own clock -- the
# one cargo compares against -- is what makes a sync mean something.
find . -name '*.rs' -not -path './target/*' -exec touch {} + 2> /dev/null
find . -name '*.toml' -not -path './target/*' -exec touch {} + 2> /dev/null

# The harness drives the real binaries; the Ubuntu guest's run phase has a
# cache of its own where the gate never built them.
echo "== binaries"
cargo build --release --locked -p rue -p rued || exit 1

echo "== tier 5"
# One test at a time: the host's state is global. RUE_E2E=1 is what the
# harness's tests demand, and is set here and nowhere else.
RUE_E2E=1 cargo test -p rue-e2e --release --locked -- --test-threads=1
