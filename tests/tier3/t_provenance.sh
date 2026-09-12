#!/bin/sh
# Self-test of the provenance guard: it must pass on a tree whose recorded
# files still match the commit they name, and fail on every way one can stop
# matching -- an edited file, a file added to a vector after the fact, a moved
# tag, a fixture touched since it was recorded, a directory with no section --
# and refuse to run at all where it cannot read a history.
#
# The trees are built here, with `git init` and real commits, rather than
# borrowed from the repository: a guard that reads git must be tested against
# git, and a scratch repository is the only way to move a tag or rewrite a
# fixture without touching this one. The repository's own answer is a separate
# assertion at the end.
set -u

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd) || exit 2
guard=$root/tools/lint-provenance.sh

command -v git > /dev/null 2>&1 || {
    echo "t_provenance: no git; the guard under test reads a repository" >&2
    exit 2
}

tmp=$(mktemp -d "${TMPDIR:-/tmp}/rue-t-prov.XXXXXX") || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM

rc=0
ok()  { echo "ok      $1"; }
bad() { echo "not ok  $1" >&2; rc=1; }

# git refuses to commit without an identity, and the environment's must not
# leak into a test: every invocation carries its own.
g() { # g <dir> <args...>
    d=$1
    shift
    git -C "$d" -c user.name=rue-selftest -c user.email=selftest@example.invalid \
        -c commit.gpgsign=false -c init.defaultBranch=main "$@"
}

# A tree shaped like the repository's: tenants/<t>/plan.rue committed and
# tagged, then copied into tenants/_upgrade/<rel>/ as a vector, plus a
# fixtures directory recorded as `committed`.
make_tree() { # make_tree <dir>
    d=$1
    mkdir -p "$d/tenants/t1" "$d/engine/tests/fixtures/store-v0.1.0" || exit 2
    g "$d" init -q . > /dev/null 2>&1 || exit 2

    printf 'plan "one" {\n    step "a"\n}\n' > "$d/tenants/t1/plan.rue"
    printf 'host = "a"\n' > "$d/tenants/t1/inventory.toml"
    g "$d" add -A > /dev/null 2>&1
    g "$d" commit -q -m "the release" > /dev/null 2>&1 || exit 2
    g "$d" tag -a v0.1.0 -m v0.1.0 > /dev/null 2>&1 || exit 2
    commit=$(g "$d" rev-parse "v0.1.0^{commit}")

    mkdir -p "$d/tenants/_upgrade/v0.1.0/t1"
    cp "$d/tenants/t1/plan.rue" "$d/tenants/_upgrade/v0.1.0/t1/plan.rue"
    cp "$d/tenants/t1/inventory.toml" "$d/tenants/_upgrade/v0.1.0/t1/inventory.toml"
    cat > "$d/tenants/_upgrade/PROVENANCE" <<EOF
[v0.1.0]
kind = tag
repo = example.invalid/selftest/rue
ref = v0.1.0
commit = $commit
prefix = tenants
made = 2026-09-12
EOF

    printf '1\n' > "$d/engine/tests/fixtures/store-v0.1.0/schema"
    printf '{"e":"Applied"}\n' > "$d/engine/tests/fixtures/store-v0.1.0/journal.ndjson"
    g "$d" add -A > /dev/null 2>&1
    g "$d" commit -q -m "vectors and the fixture" > /dev/null 2>&1 || exit 2

    # THE RECORD IS ITS OWN COMMIT, and it has to be: a record naming the
    # commit that adds the fixture cannot be inside that commit, and amending
    # it in moves the very commit it names. The first draft of this test did
    # exactly that and chased its own tail, which is also why the repository's
    # own record was added after the fixtures rather than with them.
    fx=$(g "$d" rev-parse HEAD)
    cat > "$d/engine/tests/fixtures/PROVENANCE" <<EOF
[store-v0.1.0]
kind = committed
repo = example.invalid/selftest/rue
commit = $fx
made = 2026-09-12
written_by = v0.1.0
schema = 1
EOF
    g "$d" add -A > /dev/null 2>&1
    g "$d" commit -q -m "record where the fixture came from" > /dev/null 2>&1 || exit 2
}

# fails <dir> <needle> <what>  -- the guard must exit non-zero and say <needle>.
fails() {
    if out=$(sh "$guard" --root "$1" 2>&1); then
        bad "$3 (the guard passed)"
        return
    fi
    if printf '%s' "$out" | grep -q -- "$2"; then
        ok "$3"
    else
        bad "$3 (failed, but did not name $2)"
    fi
}

# 1. An untouched tree passes.
make_tree "$tmp/clean"
if sh "$guard" --root "$tmp/clean" > /dev/null 2>&1; then
    ok "a vector that matches its commit passes"
else
    sh "$guard" --root "$tmp/clean" >&2 2>&1
    bad "a vector that matches its commit passes"
fi

# 2. A vector edited by hand: the case the whole guard exists for.
make_tree "$tmp/edited"
printf 'plan "one" {\n    step "b"\n}\n' > "$tmp/edited/tenants/_upgrade/v0.1.0/t1/plan.rue"
fails "$tmp/edited" "differs from" "a vector edited after the fact fails"

# 3. A file added to a vector that was not in the tag. The release did not
#    ship it, so the vector is no longer that release's text.
make_tree "$tmp/added"
printf 'x\n' > "$tmp/added/tenants/_upgrade/v0.1.0/t1/extra.toml"
fails "$tmp/added" "does not exist at" "a file the tag never had fails"

# 4. A moved tag, reported as such: it explains every mismatch under it, so a
#    guard that reported only bytes would send the reader to the wrong place.
make_tree "$tmp/moved"
printf 'plan "one" {\n    step "c"\n}\n' > "$tmp/moved/tenants/t1/plan.rue"
g "$tmp/moved" add -A > /dev/null 2>&1
g "$tmp/moved" commit -q -m "after the release" > /dev/null 2>&1
g "$tmp/moved" tag -f -a v0.1.0 -m moved > /dev/null 2>&1
fails "$tmp/moved" "the tag moved" "a tag that moved off the recorded commit fails"

# 5. A store fixture edited in the working tree.
make_tree "$tmp/fixture"
printf '{"e":"Reverted"}\n' > "$tmp/fixture/engine/tests/fixtures/store-v0.1.0/journal.ndjson"
fails "$tmp/fixture" "differs from" "a store fixture edited in the tree fails"

# 6. A store fixture edited AND committed: the bytes match their new commit,
#    so only the last-touched check catches it.
make_tree "$tmp/recommitted"
printf '{"e":"Reverted"}\n' > "$tmp/recommitted/engine/tests/fixtures/store-v0.1.0/journal.ndjson"
g "$tmp/recommitted" add -A > /dev/null 2>&1
g "$tmp/recommitted" commit -q -m "quietly rewrite a fixture" > /dev/null 2>&1
fails "$tmp/recommitted" "was last touched by" "a store fixture rewritten and committed fails"

# 7. The schema a fixture claims and the schema it carries must agree.
make_tree "$tmp/schema"
printf '2\n' > "$tmp/schema/engine/tests/fixtures/store-v0.1.0/schema"
g "$tmp/schema" add -A > /dev/null 2>&1
g "$tmp/schema" commit -q -m "bump the fixture's schema file" > /dev/null 2>&1
fails "$tmp/schema" "schema says 1" "a fixture whose schema file disagrees with its record fails"

# 8. A vector directory with no section at all -- the silence this guard is
#    here to convert into a failure.
make_tree "$tmp/unrecorded"
mkdir -p "$tmp/unrecorded/tenants/_upgrade/v0.9.0/t1"
printf 'x\n' > "$tmp/unrecorded/tenants/_upgrade/v0.9.0/t1/plan.rue"
fails "$tmp/unrecorded" "has no section" "a vector directory with no record fails"

# 9. A record naming an abbreviated commit is not a record.
make_tree "$tmp/abbrev"
sed 's/^commit = \(.......\).*/commit = \1/' \
    "$tmp/abbrev/tenants/_upgrade/PROVENANCE" > "$tmp/abbrev/tenants/_upgrade/P.new"
mv "$tmp/abbrev/tenants/_upgrade/P.new" "$tmp/abbrev/tenants/_upgrade/PROVENANCE"
fails "$tmp/abbrev" "not 40" "an abbreviated commit is refused"

# 9b. TWO vectors, each internally consistent, each labelled as the other.
#     Every byte check passes -- each directory really does match the commit
#     its own record names -- and only the directory-is-a-label rule catches
#     it. This case exists because the room's rule says a fixture with ONE of
#     something tests fewer rules than it appears to: every case above has a
#     single vector, so none of them can tell a vector from the WRONG vector.
make_tree "$tmp/swapped"
d=$tmp/swapped
printf 'plan "one" {\n    step "later"\n}\n' > "$d/tenants/t1/plan.rue"
g "$d" add -A > /dev/null 2>&1
g "$d" commit -q -m "the second release" > /dev/null 2>&1
g "$d" tag -a v0.2.0 -m v0.2.0 > /dev/null 2>&1
two=$(g "$d" rev-parse "v0.2.0^{commit}")
one=$(g "$d" rev-parse "v0.1.0^{commit}")

# v0.2.0's directory, built correctly from its own tag.
mkdir -p "$d/tenants/_upgrade/v0.2.0/t1"
g "$d" show "v0.2.0:tenants/t1/plan.rue" > "$d/tenants/_upgrade/v0.2.0/t1/plan.rue"
g "$d" show "v0.2.0:tenants/t1/inventory.toml" > "$d/tenants/_upgrade/v0.2.0/t1/inventory.toml"

# Now swap the two directories' CONTENT and their records together, so each
# directory matches the commit it names and both are mislabelled.
g "$d" show "v0.2.0:tenants/t1/plan.rue" > "$d/tenants/_upgrade/v0.1.0/t1/plan.rue"
g "$d" show "v0.1.0:tenants/t1/plan.rue" > "$d/tenants/_upgrade/v0.2.0/t1/plan.rue"
cat > "$d/tenants/_upgrade/PROVENANCE" <<EOF
[v0.1.0]
kind = tag
repo = example.invalid/selftest/rue
ref = v0.2.0
commit = $two
prefix = tenants
made = 2026-09-12

[v0.2.0]
kind = tag
repo = example.invalid/selftest/rue
ref = v0.1.0
commit = $one
prefix = tenants
made = 2026-09-12
EOF
fails "$d" "is its label" "two vectors labelled as each other fail, though every byte matches"

# 10. A tree with no repository is exit 2, not a pass: a guard that cannot see
#     a history has not checked anything, and saying so is the whole point.
mkdir -p "$tmp/norepo/tenants/_upgrade" "$tmp/norepo/engine/tests/fixtures"
: > "$tmp/norepo/tenants/_upgrade/PROVENANCE"
: > "$tmp/norepo/engine/tests/fixtures/PROVENANCE"
sh "$guard" --root "$tmp/norepo" > /dev/null 2>&1
code=$?
if [ "$code" -eq 2 ]; then
    ok "a tree that is not a work tree is exit 2"
else
    bad "a tree that is not a work tree is exit 2 (got $code)"
fi

# 11. And the repository itself agrees.
if sh "$guard" --root "$root" > /dev/null 2>&1; then
    ok "the repository's vectors and fixtures match the commits they name"
else
    bad "the repository's vectors and fixtures match the commits they name"
fi

exit "$rc"
