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

# Which stages this guest can run. Every file in tenants/e2e/tests runs by
# default, so a new stage needs no edit here to be picked up; a stage may
# be conditional only by being NAMED below, with the requirement it needs.
# What is not run is printed, because a stage that quietly stops running on
# every guest is indistinguishable from one that passes.
stages=''
withheld=''
for f in tenants/e2e/tests/*.rs; do
    name=${f##*/}
    name=${name%.rs}
    case $name in
        reactive)
            # T4's reactive host IS an Elixir process (8.4), and the plan
            # of record runs T4 on the Ubuntu guest. This is not a skip
            # around a failure: there is nothing for the stage to drive
            # here. If elixir ever goes missing from the guest that is
            # meant to have it, sdk/conform-all.sh fails first and loudly
            # -- a missing interpreter is a failure there, never a skip --
            # so T4 cannot vanish from every guest unnoticed.
            if command -v mix > /dev/null 2>&1; then
                stages="$stages $name"
            else
                withheld="$withheld $name(no elixir on this guest)"
            fi
            ;;
        succession)
            # T2's guests are jails and its rollback knell acts on ZFS: the
            # stage needs jail(8), which is FreeBSD's. Nothing here is
            # simulated on a host without it -- a jail stood in for by
            # something else would prove the stand-in.
            if [ "$(uname -s)" = FreeBSD ] && command -v jls > /dev/null 2>&1; then
                stages="$stages $name"
            else
                withheld="$withheld $name(no jail(8) on this guest)"
            fi
            ;;
        *) stages="$stages $name" ;;
    esac
done

echo "== tier 5:$stages"
if [ -n "$withheld" ]; then
    echo "== not run here:$withheld"
fi
if [ -z "$stages" ]; then
    echo "run.sh: no stage can run on this guest" >&2
    exit 1
fi

select=''
for name in $stages; do
    select="$select --test $name"
done

# One test at a time: the host's state is global. RUE_E2E=1 is what the
# harness's tests demand, and is set here and nowhere else.
# shellcheck disable=SC2086
RUE_E2E=1 cargo test -p rue-e2e --release --locked $select -- --test-threads=1
