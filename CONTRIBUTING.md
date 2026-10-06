# Contributing to slakio

Thanks for your interest. slakio is in early development and changes quickly, so for anything
larger than a small fix, please open an issue first to agree on the approach.

## Build

You need the Rust toolchain of `rust-toolchain.toml` (rustup installs it on the first `cargo`
command).

```sh
cargo build
cargo run -p slakio-tui -- --help
```

## Test

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI also runs these on Linux, macOS and Windows, plus `cargo deny check`, `cargo audit`, the
third-party notices, actionlint, the repository rules (`.github/scripts/repo-rules.sh`), the
dependency direction check (`.github/scripts/dependency-direction.sh`) and the performance
budgets (`docs/perf.md`).

- Tests never touch your real credentials or settings: they keep credentials in memory
  (`SLAKIO_SECRET_STORE=memory`) and point every directory at a scratch folder. Do the same when
  you run the binary for development: `SLAKIO_SECRET_STORE=memory XDG_CONFIG_HOME=/tmp/s/config
  XDG_DATA_HOME=/tmp/s/data XDG_STATE_HOME=/tmp/s/state XDG_CACHE_HOME=/tmp/s/cache cargo run -p
  slakio-tui`.
- Unit tests live next to the code in `<module>/tests.rs`; integration tests in each crate's
  `tests/`. Test names are sentences that say what must hold.
- Screens are pinned with [insta](https://insta.rs) snapshots in English. After an intended
  rendering change run `INSTA_UPDATE=always cargo test -p slakio-tui` and review the snapshot
  diff as part of the change.
- `docs/keybindings.md` is generated from the key map:
  `SLAKIO_BLESS=1 cargo test -p slakio-tui --test keybindings_doc`.
- UI text goes through the catalogs in `locales/` (`en.toml` is the source of truth; every
  locale must have the same keys). See "UI strings (i18n)" in
  [docs/architecture.md](docs/architecture.md).
- Test data is invented. Never commit real tokens, cookies, workspace or user names, e-mail
  addresses or message text. `scripts/hooks/pre-commit` checks staged files for the usual
  shapes; install it with `git config core.hooksPath scripts/hooks`. If you keep a private list
  of names that must never be committed, point `SLAKIO_PRIVATE_DENYLIST` at it (one entry per
  line); the hook then refuses commits that contain any of them.

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/):
  `feat: …`, `fix: …`, `docs: …`, `test: …`, `refactor: …`, `perf: …`, `build: …`, `ci: …`,
  `chore: …`, with an optional scope (`fix(tui): …`).
- Keep a pull request to one change, with tests for what it fixes or adds. Pull requests are
  squash-merged.
- A `refactor:` commit changes no behavior, so it changes no snapshot under
  `tests/snapshots/` (CI checks every pushed commit, `.github/scripts/history-rules.sh`).
- The guards are changed on purpose only. A commit that changes the size limit of
  `.github/scripts/file-size.sh`, a script of `.github/scripts/` that checks the repository,
  `clippy.toml`, the lint levels of a `Cargo.toml` (the workspace's `[workspace.lints]`, a
  crate's `[lints]`) or
  `.github/workflows/ci.yml` says why in a trailer,
  `Guard-change: <reason>` (CI fails without it), and a pull request that does names each
  changed file in its description. The file-size allowlist holds a path and a whole number of
  lines per entry, each path once, and may only be lowered, never raised or added to, in any commit
  (`.github/scripts/test-guards.sh` tests these rules).
- The code, comments and documentation are in English; Korean text only appears as the values
  of `locales/ko.toml` (CI checks this).

## Releases

There is no release yet. `.github/workflows/release.yml` builds and checks the release
archives and the Homebrew formula on every change to the release files and on a manual run,
without publishing anything. Publishing needs a `v<version>` tag and the
`HOMEBREW_TAP_DEPLOY_KEY` repository secret; without the secret a tag run stops before building
anything.

When releases start:

1. Set `version` in `[workspace.package]` of `Cargo.toml`, run `cargo build` so that
   `Cargo.lock` follows, and merge that change.
2. Tag that commit `v<version>` and push the tag.

The workflow fails unless the tag is exactly `v` and the workspace version. It builds the
archives (macOS and Linux, arm64 and x86_64; Windows x86_64), installs and tests the Homebrew
formula from them, publishes the GitHub release, then updates `Formula/slakio.rb` in
[DokaDev/homebrew-tap](https://github.com/DokaDev/homebrew-tap).

## License

By contributing, you agree that your contributions are dual-licensed under the MIT and
Apache-2.0 licenses, as the rest of the project.
