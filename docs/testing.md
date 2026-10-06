# Testing

| Layer | Where | What |
|---|---|---|
| Unit | `<module>/tests.rs` | pure logic: paths, atomic writes, config, secret store, catalog rules, key map and its conflict checker, shell state, screen geometry, theme, terminal restore sequences |
| Catalog | `crates/slakio-core/tests/catalog.rs` | the i18n build checks fed with fixtures, and the shipped catalogs |
| Flows and screens | `crates/slakio-tui/tests/app_flows.rs` | the app driven headless with Ratatui's `TestBackend`: keys in, frames out; insta snapshots (English only) |
| Shell flows | `crates/slakio-tui/tests/shell_flows.rs` | the shell over the demo world (the demo backend pumped through `exchange`, as the binary's loop does): keys and mouse in, frames out, at 80×24, 120×40 and 200×50 |
| Pane flows | `crates/slakio-tui/tests/pane_flows.rs` | the conversation pane, thread panel, VISUAL copy, composer and history over the demo world; snapshots of the requirements' cases 1 and 2 at three sizes and of the hostile-strings channel; nothing a terminal would act on reaches a cell or the bytes written; rows laid out per frame stay within twice the visible rows on the 10k channel and the 1,200-reply thread |
| Keyboard flows | `crates/slakio-tui/tests/keys_flows.rs` | where the focus lands, `Esc` one step at a time, quitting (and its question), the keyboard help, the which-key popup and its delay, paging keys, the hint line where the keyboard is, the icons question |
| UX regressions | `crates/slakio-tui/tests/ux_flows.rs` | one test per defect found on the first look at the interface (focus after `Ctrl+W`, `Enter` without a selection, the overlay rail, underlined selections, clipped names, the composer's box, the thread's first rows, 80×24 with a thread, the wheel), each failing before its fix |
| Review findings | `crates/slakio-tui/tests/gate_flows.rs` | one test per finding of the review of the reworked interface (the help's cursor on its surface, readable text on a selection, no lone period in a question, the icons question's keys, `? help` at 80 columns, the collapsed rail's stripes, the which-key popup over a dimmed screen, dividers, the place in the status line, hints of an empty list), each failing before its fix |
| Owner feedback | `crates/slakio-tui/tests/rail_flows.rs`, `palette_flows.rs`, `presence_flows.rs`, `theme_flows.rs` | the rail by keyboard (`Ctrl+R`, `Space r`, the `Tab` round, picking inside it, hints and help), the command palette (layout, filtering, selection, running, errors in the box, mouse, every theme), DM presence by shape and color, `:theme` at once and saved (the config file's comments kept: `signals.rs`), each failing before its change |
| Styles | `crates/slakio-tui/tests/style_flows.rs` | what text snapshots cannot see, in every built-in theme and without color: nothing underlined, unfocused text keeps its color, the overlay rail leaves the list visible, one accent border, the selection is a bar across the row, no text in its background's color; snapshots with each row's styles as theme token names |
| No emoji | `crates/slakio-tui/tests/no_emoji.rs` | the UI's sources and catalogs draw no emoji |
| Sanitiser | `crates/slakio-core/src/sanitize/tests.rs` | unit cases and property tests (proptest, stable): no control, escape, bidi or invisible character survives; idempotent; caps; text around a sequence survives; the work stays linear on adversarial input (steps counted, not timed) |
| Fuzzing | `fuzz/` (nightly, `.github/workflows/fuzz.yml`) | `sanitize`: the sanitiser's invariants on arbitrary bytes, fast; `sanitize_linear`: linear work on the input repeated up to 32 KiB; both with cargo-fuzz on a schedule (not a merge gate); a crash becomes a stable regression test |
| Remote text | `crates/slakio-tui/tests/sanitised_text.rs` | the drawing code never takes remote text around the sanitiser |
| Fake world | `crates/slakio-world/src` | determinism per seed, the sizes the demo promises, history and thread pages, the hostile-string corpus and hostile names |
| Generated docs | `crates/slakio-tui/tests/keybindings_doc.rs` | `docs/keybindings.md` equals the generated text (`SLAKIO_BLESS=1` rewrites it) |
| Real binary | `crates/slakio-tui/tests/signals.rs` (Unix) | the binary in a pseudo terminal (`script`): `:qa`, SIGTERM, SIGHUP and a panic each restore the terminal; a failed test leaves no process behind; without `script` each test prints `SKIPPED`, and fails with `SLAKIO_REQUIRE_PTY=1` (set in CI) |
| Performance | `crates/slakio-bench` | count budgets of the release binary (`docs/perf.md`) |

Rules:

- Tests never touch the user's credentials, settings or data: the secret store is always the
  in-memory one, and binaries under test get scratch `XDG_*` directories.
- Test names are sentences that say what must hold.
- Snapshots are reviewed as part of the change that alters them (`INSTA_UPDATE=always`); insta
  never rewrites them under CI. Hangul in a screen (the demo has Korean names) is written as
  `＊` in snapshot files, which hold no Hangul like every tracked file.
- Flow tests read the app through its queries (`app/query.rs`: where the focus is, which popup
  is up, the panes and what they hold) and drive it by keys, the mouse and `select_message`;
  the work area's own state is private to the crate, so its shape can change without them.
- Every fix ships a test that fails without it. An assertion is never weakened to make a test
  pass.
- Test data is invented; nothing from a real workspace is ever committed (see CONTRIBUTING.md).
