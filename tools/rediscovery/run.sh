#!/bin/sh
# The rediscovery battery (docs/ROADMAP.md section 10.2).
#
#   sh tools/rediscovery/run.sh --tier N [--row NAME] [--suite NAME] [--keep]
#
# For each row of table.tsv in the named tier: copy the repository to a
# scratch directory, run the row's test selector there in the row's suite
# (cabal: the prototype's tasty suite; cargo: the Rust workspace; python,
# mix, maven, dotnet: a non-Rust SDK's own suite, under sdk/) and require
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
# Every toolchain the selected rows need must be on PATH, or the run refuses
# with exit 2 before any row starts: a row whose suite cannot run is not a
# row that was rediscovered. The SDK suites' toolchains live where
# sdk/test-all.sh runs (the Ubuntu reaper guest, the pipeline's images) and
# not on the FreeBSD workstation, so a tier holding SDK rows is run in two
# halves, each named with --suite: the workstation's (cabal, cargo) and the
# guest's. --suite, like --row, narrows the run and says so in its summary.
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
usage: sh tools/rediscovery/run.sh --tier N [--row NAME] [--suite NAME] [--keep]

  --tier N      the tier whose rows to run (required)
  --row NAME    only the row whose patch is NAME.patch
  --suite NAME  only the rows in suite NAME (cabal, cargo, python, mix,
                maven, dotnet); may be given more than once
  --keep        keep a failed row's scratch copy and print its path
USAGE
}

tier=''
only=''
suites=''
keep=0
while [ $# -gt 0 ]; do
    case $1 in
        --tier) [ $# -ge 2 ] || { usage >&2; exit 2; }; tier=$2; shift ;;
        --tier=*) tier=${1#--tier=} ;;
        --row) [ $# -ge 2 ] || { usage >&2; exit 2; }; only=$2; shift ;;
        --suite)
            [ $# -ge 2 ] || { usage >&2; exit 2; }
            case $2 in
                cabal|cargo|python|mix|maven|dotnet) suites="$suites $2" ;;
                *) echo "rediscovery: unknown suite: $2" >&2; exit 2 ;;
            esac
            shift ;;
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

# The data rows of one tier, comments and header dropped; with --row or
# --suite, only the rows they name.
rows() {
    awk -F "$tab" -v t="$1" -v only="$only" -v suites="$suites " '
        /^#/ { next }
        NF < 6 { next }
        $1 == "patch-file" { next }
        $2 != t { next }
        only != "" && $1 != only ".patch" { next }
        suites != " " && index(suites, " " $4 " ") == 0 { next }
        { print }
    ' "$table"
}

rowfile=$(mktemp "${TMPDIR:-/tmp}/rue-rediscover-rows.XXXXXX") || exit 2
trap 'rm -f "$rowfile"' EXIT INT TERM
rows "$tier" > "$rowfile"
if [ ! -s "$rowfile" ]; then
    if [ -n "$only" ] || [ -n "$suites" ]; then
        echo "rediscovery: no row in tier $tier matches${only:+ --row $only}${suites:+ --suite$suites}" >&2
    else
        echo "rediscovery: no rows for tier $tier in $table" >&2
    fi
    echo "rediscovery: FAIL (a battery with nothing in it proves nothing)"
    exit 2
fi

# needs <suite>: the commands that suite's rows run.
needs() {
    case $1 in
        cabal) echo "cabal ghc" ;;
        cargo) echo cargo ;;
        python) echo python3 ;;
        mix) echo "mix elixir" ;;
        maven) echo "mvn java" ;;
        dotnet) echo dotnet ;;
    esac
}
# patch(1) applies every row. Without it each row would report that its
# patch "did not apply", blaming the protection for a missing tool.
missing=''
command -v patch > /dev/null 2>&1 || missing=" patch (every row)"
used=$(awk -F "$tab" '{ print $4 }' "$rowfile" | sort -u)
for s in $used; do
    for t in $(needs "$s"); do
        command -v "$t" > /dev/null 2>&1 || missing="$missing $t ($s)"
    done
done
if [ -n "$missing" ]; then
    echo "rediscovery: not on PATH, and the selected rows need them:$missing" >&2
    echo "rediscovery: run those rows where their toolchain is, or narrow with --suite" >&2
    exit 2
fi

pass=0
fail=0

# run_suite <suite> <copy> <selector> <env> <log>  -- run the selector in a
# copy; the exit status is the suite runner's.
# shellcheck disable=SC2086
#   Deliberate word splitting: the row's env column is VAR=VALUE assignments,
#   and env(1) is what takes them.
run_suite() {
    case $1 in
        cabal) ( cd "$2/proto" && env $4 cabal test all --builddir "$2/proto/dist-newstyle" --test-show-details=direct --test-options="-p $3" ) > "$5" 2>&1 ;;
        cargo) ( cd "$2" && env $4 CARGO_TARGET_DIR="$2/target" cargo test --workspace --locked -- "$3" ) > "$5" 2>&1 ;;
        python) ( cd "$2/sdk/python" && env $4 python3 -m unittest discover -s tests -t . -v -k "$3" ) > "$5" 2>&1 ;;
        mix) ( cd "$2/sdk/elixir" && env $4 MIX_ENV=test mix test "$3" ) > "$5" 2>&1 ;;
        maven) ( cd "$2/sdk/java" && env $4 mvn -B -ntp test -Dtest="$3" -Dsurefire.failIfNoSpecifiedTests=true ) > "$5" 2>&1 ;;
        dotnet) ( cd "$2/sdk/dotnet" && env $4 dotnet test tests/RueHook.Tests --filter "$3" ) > "$5" 2>&1 ;;
        *) echo "rediscovery: unknown suite $1" >&2; return 2 ;;
    esac
}

# build_suite <suite> <copy> <log>  -- the patched tree must compile.
build_suite() {
    case $1 in
        cabal) ( cd "$2/proto" && cabal build all --builddir "$2/proto/dist-newstyle" ) > "$3" 2>&1 ;;
        cargo) ( cd "$2" && CARGO_TARGET_DIR="$2/target" cargo build --workspace --all-targets --locked ) > "$3" 2>&1 ;;
        python) ( cd "$2/sdk/python" && python3 -m compileall -q rue_hook tests ) > "$3" 2>&1 ;;
        mix) ( cd "$2/sdk/elixir" && env MIX_ENV=test mix compile --warnings-as-errors ) > "$3" 2>&1 ;;
        maven) ( cd "$2/sdk/java" && mvn -B -ntp test-compile ) > "$3" 2>&1 ;;
        dotnet) ( cd "$2/sdk/dotnet" && dotnet build tests/RueHook.Tests ) > "$3" 2>&1 ;;
        *) return 2 ;;
    esac
}

passed_count() { # passed_count <suite> <log>: tests the baseline selected
    case $1 in
        cabal) n=$(sed -n 's/^All \([0-9][0-9]*\) tests passed.*/\1/p' "$2") ;;
        cargo) n=$(awk '/^test result:/ { for (i = 1; i <= NF; i++) if ($i == "passed;") s += $(i - 1) } END { print s + 0 }' "$2") ;;
        # "Ran N tests" then "OK": unittest exits nonzero on zero tests, so
        # an empty selection already failed the baseline.
        python) n=$(sed -n 's/^Ran \([0-9][0-9]*\) tests\{0,1\} in .*/\1/p' "$2" | tail -1) ;;
        # ExUnit's summary changed wording in Elixir 1.19: "N tests, 0
        # failures" before (the guest's apt Elixir is 1.18), "Result: N
        # passed" after, which becomes "Result: P/T passed" on a failure.
        mix) n=$(sed -n -e 's/^\([0-9][0-9]*\) tests\{0,1\}, 0 failures.*/\1/p' \
                 -e 's/^Result: \([0-9][0-9]*\) passed.*/\1/p' "$2" | tail -1) ;;
        # Surefire's closing total, the line with no "-- in <class>".
        maven) n=$(grep 'Tests run: [0-9]*, Failures: 0, Errors: 0, Skipped: 0$' "$2" | tail -1 | sed 's/.*Tests run: \([0-9]*\),.*/\1/') ;;
        dotnet) n=$(sed -n 's/^Passed! *- Failed: *0, Passed: *\([0-9][0-9]*\),.*/\1/p' "$2" | tail -1) ;;
        *) n=0 ;;
    esac
    [ -n "$n" ] || n=0
    echo "$n"
}

failed_evidence() { # failed_evidence <suite> <log>: the run reported failed tests
    case $1 in
        cabal) grep -q 'tests failed' "$2" ;;
        cargo) grep -q '^test result: FAILED' "$2" ;;
        python) grep -q '^FAILED (' "$2" ;;
        mix) grep -Eq '^[0-9]+ tests?, [1-9][0-9]* failures?|^Failed: [1-9]' "$2" ;;
        maven) grep -Eq 'Tests run: [0-9]+, (Failures: [1-9]|Failures: [0-9]+, Errors: [1-9])' "$2" ;;
        dotnet) grep -Eq '^Failed! +- Failed: +[1-9]' "$2" ;;
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
    echo "== $patch: expecting $suite '$selector' to fail (stage $stage, env $rowenv)"
    if [ ! -r "$here/patches/$patch" ]; then
        echo "   FAIL: no such patch: $here/patches/$patch"
        fail=$((fail + 1))
        return 0
    fi

    scratch=$(mktemp -d "${TMPDIR:-/tmp}/rue-rediscover.XXXXXX") || return 1

    # A copy, not a checkout, in two commands so each answers for itself.
    # The SDKs' build output is left behind for the reason the target
    # directory is: a baseline must be built from the copy's own sources.
    if ! ( cd "$root" && tar -cf "$scratch.tar" --exclude ./.git --exclude ./out --exclude ./proto/dist-newstyle --exclude ./.cabal_cache --exclude ./target --exclude ./.cargo_cache \
        --exclude ./sdk/elixir/_build --exclude ./sdk/elixir/deps --exclude ./sdk/java/target --exclude ./sdk/java/build \
        --exclude './sdk/dotnet/*/*/bin' --exclude './sdk/dotnet/*/*/obj' . ); then
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
    grep -E 'FAIL$|^test .* FAILED$|^(FAIL|ERROR): |^ +[0-9]+\) test |<<< (FAILURE|ERROR)!|^ +Failed ' "$log" | sed 's/^ */         /'
    rm -rf "$scratch"
    pass=$((pass + 1))
    return 0
}

# Each row runs with stdin from /dev/null. The loop reads the rows on
# stdin, and a suite that reads its own -- mix under Elixir 1.18 and OTP 27
# does -- swallowed the rest of the list: the remaining rows never ran, and
# the battery reported success over the ones that had.
while IFS="$tab" read -r c1 c2 c3 c4 c5 c6; do
    run_row "$c1" "$c3" "$c4" "$c5" "$c6" < /dev/null || { echo "rediscovery: could not create a scratch copy" >&2; exit 2; }
    : "$c2"
done < "$rowfile"

# And every selected row must have been judged: a row that never ran is not
# one that was rediscovered, whatever swallowed it.
selected=$(wc -l < "$rowfile" | tr -d ' ')
if [ "$((pass + fail))" -ne "$selected" ]; then
    echo "rediscovery: FAIL: $((pass + fail)) of $selected selected rows were judged; the rest never ran"
    fail=$((selected - pass))
fi
if [ -n "$only" ] || [ -n "$suites" ]; then
    echo "$pass rediscovered, $fail not (tier $tier narrowed to${only:+ row $only}${suites:+ suite$suites})"
else
    echo "$pass rediscovered, $fail not"
fi
[ "$fail" -eq 0 ] && [ "$pass" -ge 1 ]
