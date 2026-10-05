#!/bin/sh
# Writes THIRD-PARTY-NOTICES.html: the licenses of every crate built into the `slakio` binary
# (the dependency tree of crates/slakio-tui, through cargo-about with about.toml and about.hbs).
# Needs cargo-about: `cargo install --locked cargo-about --features cli`.
# Usage: scripts/third-party-notices.sh [output file]
set -eu
cd "$(dirname "$0")/.."
out=${1:-THIRD-PARTY-NOTICES.html}
cargo about generate --manifest-path crates/slakio-tui/Cargo.toml --locked --fail about.hbs > "$out"
echo "wrote $out"
