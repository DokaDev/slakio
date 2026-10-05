#!/usr/bin/env bash
# The dependency rules of docs/architecture.md, checked on the normal (non-dev, non-build)
# dependency tree of each crate (CI job `dependency-direction`):
#   - slakio-core depends on no other slakio crate, and on no UI, network or database crate;
#   - an adapter (slakio-slack, slakio-auth, slakio-store, slakio-world) depends on no slakio
#     crate but slakio-core;
#   - no crate depends on slakio-fake outside its dev-dependencies.
# Crates that do not exist yet are skipped.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

members=$(cargo metadata --no-deps --locked --format-version 1 | jq -r '.packages[].name')
deps() {
  cargo tree -p "$1" -e normal --prefix none --locked | awk '{print $1}' | sort -u | grep -vx "$1" || true
}
failed=0
fail() {
  echo "$1" >&2
  failed=1
}

core=$(deps slakio-core)
for d in $(grep '^slakio-' <<< "$core" || true); do
  fail "slakio-core depends on $d"
done
for d in ratatui crossterm mio reqwest hyper tungstenite tokio-tungstenite rusqlite libsqlite3-sys ureq; do
  if grep -qx "$d" <<< "$core"; then
    fail "slakio-core depends on $d (UI, network or database)"
  fi
done

for adapter in slakio-slack slakio-auth slakio-store slakio-world; do
  grep -qx "$adapter" <<< "$members" || continue
  for d in $(deps "$adapter" | grep '^slakio-' | grep -vx slakio-core || true); do
    fail "$adapter depends on $d (adapters depend on slakio-core only)"
  done
done

for m in $members; do
  if deps "$m" | grep -qx slakio-fake; then
    fail "$m depends on slakio-fake outside its dev-dependencies"
  fi
done

if [[ $failed == 1 ]]; then
  exit 1
fi
echo "dependency direction: ok"
