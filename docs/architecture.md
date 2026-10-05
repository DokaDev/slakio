# Architecture

slakio is a Cargo workspace. This document says how the code is laid out and which rules hold
across it. It grows with the code; nothing here describes something that does not exist yet
unless it says so.

## Crates

| Crate | What | Depends on |
|---|---|---|
| `slakio-core` | UI-free and network-free core: the domain model, the backend protocol (commands and events tagged with a generation), paths, atomic file writes, the config file, the secret store interface, faults, i18n | nothing of slakio |
| `slakio-world` | a seeded, deterministic fake world (two workspaces, ~300 channels, DMs, a Slack Connect channel, a 10k-message channel, a 1,200-reply thread, hostile strings) for the demo mode and the tests | `slakio-core` |
| `slakio-tui` | the terminal UI (Ratatui) and the `slakio` binary | `slakio-core` |
| `slakio-bench` | performance budgets (not product code) | runs the binary |

Planned crates join with the step that needs them: `slakio-slack` (the Slack adapter), `slakio-auth`
(sign-in), `slakio-store` (the local cache), `slakio-fake` (a fake Slack server, tests only)
and `slakio-scrub` (a tool that scrubs captured API traffic into test fixtures).

### Dependency rules

```
tui -> core <- slack, auth, store, world
```

- `slakio-core` depends on no other slakio crate and on no UI, network or database crate
  (ratatui, crossterm, tokio's network parts, HTTP or websocket clients, rusqlite).
- Adapters (`slack`, `auth`, `store`, `world`) depend only on `slakio-core`, never on each
  other or on `slakio-tui`.
- The concrete adapter types appear in one wiring file of `slakio-tui`.
- `slakio-fake` is only ever a dev-dependency.

CI checks these with `cargo tree` (`.github/scripts/dependency-direction.sh`).

## Module rules

- `foo.rs` with its submodules in `foo/`, never `mod.rs`.
- Unit tests in `foo/tests.rs` (`#[cfg(test)] mod tests;` in `foo.rs`); integration tests in
  the crate's `tests/`.
- Every file opens with a comment that says what it is for and why it is shaped that way.

## The UI: state, actions, effects

`App` (`crates/slakio-tui/src/app.rs`) is a thin router. Each part of the state is a sub-state
that owns its data and its update: today the command line (`app/cmdline.rs`) and the status
line's notices (`app/status.rs`); the shell (rail, list panel), the layout (tabs and panes), the
panes, the read model of the workspaces, the input mode and the overlays join as they are
built. Rules:

- An `Action` is namespaced by its owner (`Action::CommandLine(CommandLineAction::Run)`), so
  dispatching one is routing, not one match over every action of the app.
- One action registry (`action.rs`) gives every action its id, label and `:` commands; the key
  map, the command line and `docs/keybindings.md` all read it.
- State does no I/O. The binary owns the terminal and a single `tokio::select!` loop over
  input, signals and the app's next deadline; it redraws only after something changed and
  sleeps when nothing happens (the idle wakeups are a performance budget).
- Channels from background tasks are bounded wherever a stream can be large (websocket events,
  history pages, downloads); events carry their target and a generation, and stale ones are
  dropped.

## Robustness

- **Unknown is not absent.** A file, directory or store that cannot be read is an error, never
  "empty": a config file that cannot be used is reported and never written over; a secret store
  that fails says so instead of answering "no entry".
- **Fail closed.** `SLAKIO_SECRET_STORE` with a value that is not `keychain` or `memory` stops
  the program (exit code 2); a typo never reaches the real keychain. Stores never fall back to
  one another.
- **Terminal restored exactly once.** Every terminal mode the binary turns on is recorded in one
  `TermState` and undone once, by a guard (normal exit, error, failed setup) or by the panic hook
  (before the panic message), whichever comes first. SIGTERM, SIGHUP, SIGINT and SIGQUIT end the
  loop the same way as `:qa`. A pseudo-terminal test checks each path.
- **Atomic writes.** Files are written to a temporary file, flushed and renamed
  (`fsutil::atomic_write`); a new file never replaces one that appeared meanwhile
  (`fsutil::create_new`, `fsutil::rename_new`).
- **Failures as data.** Core never words a failure: a `Fault` has a kind the UI turns into a
  catalog message and a raw detail that only goes to `<state>/errors.log`. The status line never
  shows OS or parser text.

## UI strings (i18n)

`locales/en.toml` is the source catalog; `locales/ko.toml` must have exactly the same keys and
placeholders. `crates/slakio-core/build.rs` checks both and generates a typed API: `Label` for
entries without placeholders, `Msg` for entries with typed arguments (`{count}` is a number,
`{elapsed}` a duration, anything else text). A missing key or argument is a compile error. UI
chrome takes `Localized` text, which only the catalog or the grep-able escape hatch
`Localized::verbatim` (data, key names, the product name) can produce. Korean text appears only
as the values of `locales/ko.toml`; a Korean value equal to the English one must be listed with
its reason in `SAME_IN_KO` (`crates/slakio-core/tests/catalog.rs`).

## Paths

See `crates/slakio-core/src/paths.rs`: config, data, state and cache directories, each with a
`SLAKIO_*_DIR` override and the XDG variables honored on every OS, so development runs and tests
pointed at scratch directories never touch the real ones.
