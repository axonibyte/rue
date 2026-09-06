#!/bin/sh
# The rediscovery battery (docs/ROADMAP.md section 10.2).
#
#   sh tools/rediscovery/run.sh --tier N [--row NAME] [--keep]
#
# For each row of table.tsv in the named tier: copy the repository to a
# scratch directory, run the row's test selector there in the row's suite
# (cabal: the prototype's tasty suite; cargo: the Rust workspace) and require
# it to pass with at least one test (the baseline), apply the patch that
# REVERTS one protection, require the patched tree to compile (a type error is
# not a rediscovery), run the selector again and require it to FAIL. The
# working tree is never touched. Run before a milestone is trusted, never
# automatically. Each cargo row builds into its own target directory inside
# its scratch copy: a target directory shared across copies let one row's
# artifacts answer for another's baseline once, and a battery that can
# misreport a baseline is worth less than the minutes the sharing saved.
#
# THE FALSE-PASS TRAP: this runner asserts a failure, so every way of not
# running the tests at all looks like success. Each row therefore proves, in
# order, that the selector selects something, that the patch applied, that
# the patched tree builds, and that the failing run reports "tests failed" --
# before a non-zero exit counts as a rediscovery.
#
# Exit 0 when every row of the tier was rediscovered, 1 when any was not, 2
# on a usage or contract error. Prints "N rediscovered, M not" last.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
here=$root/tools/rediscovery
table=$here/table.tsv

tab=$(printf '\t.')
tab=${tab%.}

usage() {
    cat <<USAGE
usage: sh tools/rediscovery/run.sh --tier N [--row NAME] [--keep]

  --tier N    the tier whose rows to run (required)
  --row NAME  only the row whose patch is NAME.patch
  --keep      keep a failed row's scratch copy and print its path
USAGE
}

tier=''
only=''
keep=0
while [ $# -gt 0 ]; do
    case $1 in
        --tier) [ $# -ge 2 ] || { usage >&2; exit 2; }; tier=$2; shift ;;
        --tier=*) tier=${1#--tier=} ;;
        --row) [ $# -ge 2 ] || { usage >&2; exit 2; }; only=$2; shift ;;
        --keep) keep=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "rediscovery: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done
case $tier in
    ''|*[!0-9]*) echo "rediscovery: --tier N is required" >&2; exit 2 ;;
esac
[ -r "$table" ] || { echo "rediscovery: no table at $table" >&2; exit 2; }

if ! command -v cabal > /dev/null 2>&1 || ! command -v ghc > /dev/null 2>&1; then
    echo "rediscovery: ghc/cabal are not on PATH" >&2
    exit 2
fi
if ! command -v cargo > /dev/null 2>&1; then
    echo "rediscovery: cargo is not on PATH" >&2
    exit 2
fi

# The data rows of one tier, comments and header dropped.
rows() {
    awk -F "$tab" -v t="$1" '
        /^#/ { next }
        NF < 6 { next }
        $1 == "patch-file" { next }
        $2 == t { print }
    ' "$table"
}

rowfile=$(mktemp "${TMPDIR:-/tmp}/rue-rediscover-rows.XXXXXX") || exit 2
trap 'rm -f "$rowfile"' EXIT INT TERM
rows "$tier" > "$rowfile"
if [ ! -s "$rowfile" ]; then
    echo "rediscovery: no rows for tier $tier in $table" >&2
    echo "rediscovery: FAIL (a battery with nothing in it proves nothing)"
    exit 2
fi

pass=0
fail=0
skippedrows=0

# run_suite <suite> <copy> <selector> <env> <log>  -- run the selector in a
# copy; the exit status is the suite runner's.
# shellcheck disable=SC2086
#   Deliberate word splitting: the row's env column is VAR=VALUE assignments,
#   and env(1) is what takes them.
run_suite() {
    case $1 in
        cabal) ( cd "$2/proto" && env $4 cabal test all --builddir "$2/proto/dist-newstyle" --test-show-details=direct --test-options="-p $3" ) > "$5" 2>&1 ;;
        cargo) ( cd "$2" && env $4 CARGO_TARGET_DIR="$2/target" cargo test --workspace --locked -- "$3" ) > "$5" 2>&1 ;;
        *) echo "rediscovery: unknown suite $1" >&2; return 2 ;;
    esac
}

# build_suite <suite> <copy> <log>  -- the patched tree must compile.
build_suite() {
    case $1 in
        cabal) ( cd "$2/proto" && cabal build all --builddir "$2/proto/dist-newstyle" ) > "$3" 2>&1 ;;
        cargo) ( cd "$2" && CARGO_TARGET_DIR="$2/target" cargo build --workspace --all-targets --locked ) > "$3" 2>&1 ;;
        *) return 2 ;;
    esac
}

passed_count() { # passed_count <suite> <log>: tests the baseline selected
    case $1 in
        cabal) n=$(sed -n 's/^All \([0-9][0-9]*\) tests passed.*/\1/p' "$2") ;;
        cargo) n=$(awk '/^test result:/ { for (i = 1; i <= NF; i++) if ($i == "passed;") s += $(i - 1) } END { print s + 0 }' "$2") ;;
        *) n=0 ;;
    esac
    [ -n "$n" ] || n=0
    echo "$n"
}

failed_evidence() { # failed_evidence <suite> <log>: the run reported failed tests
    case $1 in
        cabal) grep -q 'tests failed' "$2" ;;
        cargo) grep -q '^test result: FAILED' "$2" ;;
        *) return 1 ;;
    esac
}

row_fail() { # row_fail <scratch> <message...>
    s=$1
    shift
    echo "   FAIL: $*"
    fail=$((fail + 1))
    if [ "$keep" -eq 1 ]; then
        echo "         kept: $s"
    else
        rm -rf "$s"
    fi
}

run_row() { # run_row <patch> <stage> <suite> <selector> <env>
    patch=$1
    stage=$2
    suite=$3
    selector=$4
    rowenv=$5
    if [ -n "$only" ] && [ "$patch" != "$only.patch" ]; then
        skippedrows=$((skippedrows + 1))
        return 0
    fi
    echo "== $patch: expecting $suite '$selector' to fail (stage $stage, env $rowenv)"
    if [ ! -r "$here/patches/$patch" ]; then
        echo "   FAIL: no such patch: $here/patches/$patch"
        fail=$((fail + 1))
        return 0
    fi

    scratch=$(mktemp -d "${TMPDIR:-/tmp}/rue-rediscover.XXXXXX") || return 1

    # A copy, not a checkout, in two commands so each answers for itself.
    if ! ( cd "$root" && tar -cf "$scratch.tar" --exclude ./.git --exclude ./out --exclude ./proto/dist-newstyle --exclude ./.cabal_cache --exclude ./target --exclude ./.cargo_cache . ); then
        rm -rf "$scratch" "$scratch.tar"
        echo "   FAIL: could not archive the tree"
        fail=$((fail + 1))
        return 0
    fi
    if ! ( cd "$scratch" && tar -xf "$scratch.tar" ); then
        rm -f "$scratch.tar"
        row_fail "$scratch" "could not unpack the tree into $scratch"
        return 0
    fi
    rm -f "$scratch.tar"

    log=$scratch/rediscovery.log

    # 1. Baseline: the selector passes and selects at least one test.
    if [ "$rowenv" = "-" ]; then rowenv=''; fi
    if ! run_suite "$suite" "$scratch" "$selector" "$rowenv" "$log"; then
        tail -20 "$log" | sed 's/^/         /'
        row_fail "$scratch" "the baseline run of $suite '$selector' did not pass in the unpatched copy"
        return 0
    fi
    n=$(passed_count "$suite" "$log")
    if [ "$n" -lt 1 ]; then
        row_fail "$scratch" "$suite '$selector' selects no test; a selector that runs nothing cannot fail"
        return 0
    fi

    # 2. The patch applies.
    # No fuzz: a patch whose context has drifted is a protection that moved.
    if ! ( cd "$scratch" && patch -p1 -F 0 -s < "$here/patches/$patch" ) > "$log.patch" 2>&1; then
        sed 's/^/         /' "$log.patch"
        row_fail "$scratch" "the patch did not apply; the protection it reverts has moved"
        return 0
    fi

    # 3. The patched tree compiles.
    if ! build_suite "$suite" "$scratch" "$log.build"; then
        grep -A6 'error' "$log.build" | head -30 | sed 's/^/         /'
        row_fail "$scratch" "the patched tree does not compile; a type error is not a rediscovery"
        return 0
    fi

    # 4. The selector fails, and says so.
    if run_suite "$suite" "$scratch" "$selector" "$rowenv" "$log"; then
        row_fail "$scratch" "$suite '$selector' still passed with the protection reverted ($n tests ran, none noticed)"
        return 0
    fi
    if ! failed_evidence "$suite" "$log"; then
        tail -20 "$log" | sed 's/^/         /'
        row_fail "$scratch" "the run exited non-zero without reporting failed tests"
        return 0
    fi
    echo "   PASS: rediscovered ($n tests in the baseline), by:"
    grep -E 'FAIL$|^test .* FAILED$' "$log" | sed 's/^ */         /'
    rm -rf "$scratch"
    pass=$((pass + 1))
    return 0
}

while IFS="$tab" read -r c1 c2 c3 c4 c5 c6; do
    run_row "$c1" "$c3" "$c4" "$c5" "$c6" || { echo "rediscovery: could not create a scratch copy" >&2; exit 2; }
    : "$c2"
done < "$rowfile"

if [ -n "$only" ] && [ "$((pass + fail))" -eq 0 ]; then
    echo "rediscovery: no row named $only in tier $tier" >&2
    exit 2
fi
echo "$pass rediscovered, $fail not"
[ "$fail" -eq 0 ] && [ "$pass" -ge 1 ]
