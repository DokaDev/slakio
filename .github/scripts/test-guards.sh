#!/usr/bin/env bash
# Tests of the guard scripts (file-size.sh, history-rules.sh) in throwaway repositories: each
# case builds a small history and says whether the guard must pass or fail on it. Run by CI
# (job `repo-rules`) and locally the same way.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
failed=0

# A fresh repository with the guard scripts, one Rust file of `lines` lines listed at `entry`.
repo() {
    local dir=$scratch/$1 lines=$2 entry=$3
    mkdir -p "$dir/.github/scripts" "$dir/src"
    cp "$here/file-size.sh" "$here/history-rules.sh" "$dir/.github/scripts/"
    seq "$lines" | sed 's/^/\/\/ /' >"$dir/src/big.rs"
    printf '# test\nsrc/big.rs %s\n' "$entry" >"$dir/.github/scripts/file-size-allowlist.txt"
    git -C "$dir" init -q -b main
    git -C "$dir" add -A
    git -C "$dir" -c user.name=t -c user.email=t@t -c core.hooksPath=/dev/null commit -qm "chore: start"
    echo "$dir"
}

commit() {
    local dir=$1
    shift
    git -C "$dir" add -A
    git -C "$dir" -c user.name=t -c user.email=t@t -c core.hooksPath=/dev/null commit -q "$@"
}

# Run a command inside repository `dir`.
in_repo() {
    local dir=$1
    shift
    (cd "$dir" && "$@")
}

# expect pass|fail name command…
expect() {
    local want=$1 name=$2 got=pass
    shift 2
    "$@" >"$scratch/out" 2>&1 || got=fail
    if [[ "$got" != "$want" ]]; then
        echo "FAIL $name: expected $want, got $got"
        sed 's/^/  /' "$scratch/out"
        failed=1
    else
        echo "ok   $name"
    fi
}

d=$(repo size-ok 700 700)
expect pass "an allowlisted file at its size" in_repo "$d" bash .github/scripts/file-size.sh
d=$(repo size-float 700 1200.0)
expect fail "a size that is not a whole number" in_repo "$d" bash .github/scripts/file-size.sh
d=$(repo size-word 700 big)
expect fail "a size that is not a number" in_repo "$d" bash .github/scripts/file-size.sh

d=$(repo size-twice 800 700)
printf 'src/big.rs 900\n' >>"$d/.github/scripts/file-size-allowlist.txt"
expect fail "a path listed twice" in_repo "$d" bash .github/scripts/file-size.sh

d=$(repo twice 700 700)
base=$(git -C "$d" rev-parse HEAD)
printf 'src/big.rs 700\n' >>"$d/.github/scripts/file-size-allowlist.txt"
commit "$d" -m "ci: list it again"
expect fail "an allowlist that lists a path twice" in_repo "$d" bash .github/scripts/history-rules.sh "$base"

d=$(repo raise 700 700)
base=$(git -C "$d" rev-parse HEAD)
sed -i.bak 's/ 700$/ 1200.0/' "$d/.github/scripts/file-size-allowlist.txt" && rm "$d/.github/scripts/file-size-allowlist.txt.bak"
commit "$d" -m "ci: loosen"
expect fail "an allowlist raised to a size that is not a whole number" in_repo "$d" bash .github/scripts/history-rules.sh "$base"

d=$(repo guard 700 700)
base=$(git -C "$d" rev-parse HEAD)
echo "too-many-lines-threshold = 90" >"$d/clippy.toml"
commit "$d" -m "ci: a looser threshold"
expect fail "a guard changed without a Guard-change trailer" in_repo "$d" bash .github/scripts/history-rules.sh "$base"
git -C "$d" reset -q --hard "$base"
echo "too-many-lines-threshold = 90" >"$d/clippy.toml"
commit "$d" -m "ci: a looser threshold" -m "Guard-change: measured on the whole workspace"
expect pass "a guard changed with a Guard-change trailer" in_repo "$d" bash .github/scripts/history-rules.sh "$base"

d=$(repo lints 700 700)
printf '[workspace]\n\n[workspace.lints.clippy]\ntodo = "warn"\n' >"$d/Cargo.toml"
commit "$d" -m "build: lints"
base=$(git -C "$d" rev-parse HEAD)
printf '[workspace]\n\n[workspace.lints.clippy]\ntodo = "allow"\n' >"$d/Cargo.toml"
commit "$d" -m "build: a looser lint"
expect fail "lint levels changed without a Guard-change trailer" in_repo "$d" bash .github/scripts/history-rules.sh "$base"
git -C "$d" reset -q --hard "$base"
printf '[workspace]\nmembers = ["a"]\n\n[workspace.lints.clippy]\ntodo = "warn"\n' >"$d/Cargo.toml"
commit "$d" -m "build: a member"
expect pass "Cargo.toml changed outside the lints" in_repo "$d" bash .github/scripts/history-rules.sh "$base"

d=$(repo crate-lints 700 700)
mkdir -p "$d/crates/a"
printf '[package]\nname = "a"\n\n[lints]\nworkspace = true\n' >"$d/crates/a/Cargo.toml"
commit "$d" -m "build: a crate" -m "Guard-change: a new crate with the workspace's lints"
base=$(git -C "$d" rev-parse HEAD)
printf '[package]\nname = "a"\n' >"$d/crates/a/Cargo.toml"
commit "$d" -m "build: the crate's own lints"
expect fail "a crate that drops the workspace's lints without a Guard-change trailer" in_repo "$d" bash .github/scripts/history-rules.sh "$base"
git -C "$d" reset -q --hard "$base"
printf '[package]\nname = "a"\n\n[lints.clippy]\ntodo = "allow"\n' >"$d/crates/a/Cargo.toml"
commit "$d" -m "build: a looser crate lint"
expect fail "a crate's lint levels changed without a Guard-change trailer" in_repo "$d" bash .github/scripts/history-rules.sh "$base"
git -C "$d" reset -q --hard "$base"
printf '[package]\nname = "a"\nversion = "1.0.0"\n\n[lints]\nworkspace = true\n' >"$d/crates/a/Cargo.toml"
commit "$d" -m "build: a version"
expect pass "a crate's Cargo.toml changed outside its lints" in_repo "$d" bash .github/scripts/history-rules.sh "$base"

if ((failed)); then
    exit 1
fi
echo "guard tests: ok"
