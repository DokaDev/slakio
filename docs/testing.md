# Testing

| Layer | Where | What |
|---|---|---|
| Unit | `<module>/tests.rs` | pure logic: paths, atomic writes, config, secret store, catalog rules, key map and its conflict checker, shell state, screen geometry, theme, terminal restore sequences |
| Catalog | `crates/slakio-core/tests/catalog.rs` | the i18n build checks fed with fixtures, and the shipped catalogs |
| Flows and screens | `crates/slakio-tui/tests/app_flows.rs` | the app driven headless with Ratatui's `TestBackend`: keys in, frames out; insta snapshots (English only) |
| Shell flows | `crates/slakio-tui/tests/shell_flows.rs` | the shell over the demo world (the demo backend pumped by hand, as the binary's loop does): keys and mouse in, frames out, at 80×24, 120×40 and 200×50 |
| Fake world | `crates/slakio-world/src` | determinism per seed, the sizes the demo promises, the hostile-string corpus, printable names |
| Generated docs | `crates/slakio-tui/tests/keybindings_doc.rs` | `docs/keybindings.md` equals the generated text (`SLAKIO_BLESS=1` rewrites it) |
| Real binary | `crates/slakio-tui/tests/signals.rs` (Unix) | the binary in a pseudo terminal (`script`): `:qa`, SIGTERM, SIGHUP and a panic each restore the terminal; a failed test leaves no process behind; without `script` each test prints `SKIPPED`, and fails with `SLAKIO_REQUIRE_PTY=1` (set in CI) |
| Performance | `crates/slakio-bench` | count budgets of the release binary (`docs/perf.md`) |

Rules:

- Tests never touch the user's credentials, settings or data: the secret store is always the
  in-memory one, and binaries under test get scratch `XDG_*` directories.
- Test names are sentences that say what must hold.
- Snapshots are reviewed as part of the change that alters them (`INSTA_UPDATE=always`); insta
  never rewrites them under CI.
- Every fix ships a test that fails without it. An assertion is never weakened to make a test
  pass.
- Test data is invented; nothing from a real workspace is ever committed (see CONTRIBUTING.md).
