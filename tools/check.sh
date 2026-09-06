#!/bin/sh
# The gate. Runs every phase, reports every failure, and exits 0 only if every
# phase RAN and passed.
#
# A phase whose tool is missing exits 77 and is skipped LOUDLY; it counts as a
# failure unless the caller named it in RUE_CHECK_SKIP_OK (comma-separated
# phase names). That is how a host with no GHC says so on purpose -- the
# FreeBSD reaper guest declares its skips in .reaper.toml -- and nothing is
# ever assumed about the host.
#
# This is also the reaper run command, so it must be honest about its own
# exit status: every phase's status is captured explicitly and nothing is
# piped into anything that could answer for it (dash has no pipefail).
#
# Environment:
#   RUE_CHECK_SKIP_OK   phases allowed to skip when their tool is absent
#   RUE_BUILDDIR        cabal --builddir (default proto/dist-newstyle)
#   RUE_CABAL_UPDATE=1  run `cabal update` before building (CI and reaper)
#
# shellcheck disable=SC2329
#   Every p_* function is invoked indirectly, through `phase <name> <cmd>`,
#   which shellcheck cannot follow; the functions are not dead.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd) || exit 2
cd "$root" || exit 2

RUE_BUILDDIR=${RUE_BUILDDIR:-$root/proto/dist-newstyle}
RUE_REPO_ROOT=$root
export RUE_BUILDDIR RUE_REPO_ROOT

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-check.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
failed=''
skipped=''

skip_ok() {
    case ",${RUE_CHECK_SKIP_OK:-}," in
        *",$1,"*) return 0 ;;
    esac
    return 1
}

phase() { # phase <name> <cmd...>   cmd exits: 0 ok, 77 tool missing, else fail
    name=$1
    shift
    printf '\n=== %s ===\n' "$name"
    "$@"
    st=$?
    if [ "$st" -eq 0 ]; then
        printf -- '--- ok: %s\n' "$name"
    elif [ "$st" -eq 77 ] && skip_ok "$name"; then
        printf -- '--- SKIP (declared in RUE_CHECK_SKIP_OK): %s\n' "$name"
        skipped="$skipped $name"
    elif [ "$st" -eq 77 ]; then
        printf -- '--- FAIL: %s (tool missing, and not declared skippable here)\n' "$name"
        rc=1
        failed="$failed $name"
    else
        printf -- '--- FAIL: %s (exit %s)\n' "$name" "$st"
        rc=1
        failed="$failed $name"
    fi
}

# Shell files are classified by shebang: POSIX sh files are checked with
# sh -n, bash files (ci/ scripts that run in the Debian CI image) with bash -n.
# FreeBSD base has no bash, so the bash phase may be declared skippable there.
shell_list() {
    find tools ci tests -type f -name '*.sh' > "$tmp/all.list" || return 1
    [ -s "$tmp/all.list" ] || return 1
    : > "$tmp/sh.list"
    : > "$tmp/bash.list"
    while IFS= read -r f; do
        line=''
        read -r line < "$f" 2> /dev/null
        case $line in
            '#!/bin/sh'|'#!/bin/sh '*) echo "$f" >> "$tmp/sh.list" ;;
            '#!/usr/bin/env bash'|'#!/bin/bash') echo "$f" >> "$tmp/bash.list" ;;
            *) echo "unclassified shebang in $f: $line" >&2; return 1 ;;
        esac
    done < "$tmp/all.list"
}

p_sh_syntax() {
    shell_list || { echo "no shell files found, or one has an unknown shebang" >&2; return 1; }
    st=0
    while IFS= read -r f; do
        if sh -n "$f"; then
            echo "ok      $f"
        else
            echo "SYNTAX  $f" >&2
            st=1
        fi
    done < "$tmp/sh.list"
    return "$st"
}

p_bash_syntax() {
    shell_list || { echo "no shell files found, or one has an unknown shebang" >&2; return 1; }
    [ -s "$tmp/bash.list" ] || { echo "no bash files"; return 0; }
    if ! command -v bash > /dev/null 2>&1; then
        echo "bash is not on PATH; cannot syntax-check ci/ bash scripts here" >&2
        return 77
    fi
    st=0
    while IFS= read -r f; do
        if bash -n "$f"; then
            echo "ok      $f"
        else
            echo "SYNTAX  $f" >&2
            st=1
        fi
    done < "$tmp/bash.list"
    return "$st"
}

p_shellcheck() {
    if ! command -v shellcheck > /dev/null 2>&1; then
        echo "shellcheck is not on PATH (install devel/shellcheck or apt shellcheck)" >&2
        return 77
    fi
    shell_list || { echo "no shell files found, or one has an unknown shebang" >&2; return 1; }
    st=0
    # Dialect comes from each file's shebang; -x follows sourced files.
    while IFS= read -r f; do
        shellcheck -x "$f" || st=1
    done < "$tmp/all.list"
    [ "$st" -eq 0 ] && echo "shellcheck: clean"
    return "$st"
}

p_tier3() {
    st=0
    found=0
    for t in tests/tier3/t_*.sh; do
        [ -f "$t" ] || continue
        found=1
        echo "-- $t"
        if ! sh "$t"; then
            echo "self-test failed: $t" >&2
            st=1
        fi
    done
    [ "$found" -eq 1 ] || { echo "no tier-3 self-tests found" >&2; return 1; }
    return "$st"
}

toolchain() {
    if ! command -v cabal > /dev/null 2>&1 || ! command -v ghc > /dev/null 2>&1; then
        echo "ghc/cabal are not on PATH (pkg install ghc hs-cabal-install)" >&2
        return 77
    fi
    v=$(ghc --numeric-version)
    if [ "$v" != "9.10.3" ]; then
        echo "ghc is $v; proto/cabal.project.freeze is for 9.10.3" >&2
        return 1
    fi
    return 0
}

p_cabal_build() {
    toolchain || return $?
    if [ "${RUE_CABAL_UPDATE:-0}" = 1 ]; then
        ( cd proto && cabal update ) || return 1
    fi
    ( cd proto && cabal build all --builddir "$RUE_BUILDDIR" )
}

# The suite is read-only: a checksum of everything under tenants/ and docs/
# is taken before and after, and any change fails the phase.
golden_sums() {
    : > "$1"
    for d in tenants docs; do
        [ -d "$d" ] || continue
        find "$d" -type f -exec cksum {} + >> "$1" || return 1
    done
    sort -o "$1" "$1"
}

p_cabal_test() {
    toolchain || return $?
    golden_sums "$tmp/g.before" || return 1
    ( cd proto && cabal test all --builddir "$RUE_BUILDDIR" --test-show-details=direct ) || return 1
    golden_sums "$tmp/g.after" || return 1
    if ! cmp -s "$tmp/g.before" "$tmp/g.after"; then
        echo "cabal test changed files under tenants/ or docs/; the suite is read-only" >&2
        return 1
    fi
    return 0
}

phase sh-syntax        p_sh_syntax
phase bash-syntax      p_bash_syntax
phase shellcheck       p_shellcheck
phase seam             sh tools/lint-seam.sh
phase ecodes           sh tools/lint-ecodes.sh
phase golden-hygiene   sh tools/lint-goldens.sh
phase tier3-selftests  p_tier3
phase cabal-build      p_cabal_build
phase cabal-test       p_cabal_test

printf '\n'
if [ -n "$skipped" ]; then
    printf 'skipped by declaration:%s\n' "$skipped"
fi
if [ "$rc" -ne 0 ]; then
    printf 'rue check: FAIL:%s\n' "$failed"
    exit 1
fi
printf 'rue check: PASS\n'
exit 0
