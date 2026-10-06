# Architecture

slakio is a Cargo workspace. This document says how the code is laid out and which rules hold
across it. It grows with the code; nothing here describes something that does not exist yet
unless it says so.

## Crates

| Crate | What | Depends on |
|---|---|---|
| `slakio-core` | UI-free and network-free core: the domain model, the backend protocol (commands and events tagged with a generation), the terminal-escape sanitiser, paths, atomic file writes, the config file, the secret store interface, faults, i18n | nothing of slakio |
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
that owns its data and its update: the command line (`app/cmdline.rs`), the status line's
notices (`app/status.rs`), the shell — focus between rail, list panel and work area, the rail
and list cursors, folded sections (`app/shell.rs`) —, the work area — the main pane, the auto
thread panel, which of them has the keyboard, Insert mode, back/forward history
(`app/work.rs`) — with its panes (`app/pane.rs`: loaded messages, selection, VISUAL range) and
their composers (`app/composer.rs`), the read model of the workspaces (`app/model.rs`), the
keyboard help (`app/help.rs`) and a question with two answers (`app/dialog.rs`); the layout
(tabs and splits) joins as it is built. The geometry of the screen (`screen.rs`) is one pure
function that drawing and the mouse both use. Rules:

- An `Action` is namespaced by its owner (`Action::CommandLine(CommandLineAction::Run)`), so
  dispatching one is routing, not one match over every action of the app.
- One action registry (`action.rs`) gives every action its id, label and `:` commands; the key
  map, the command line and `docs/keybindings.md` all read it.
- State does no I/O. The binary owns the terminal and a single `tokio::select!` loop over
  input, signals and the app's next deadline; it redraws only after something changed and
  sleeps when nothing happens (the idle wakeups are a performance budget).
- The UI talks to a backend only through the protocol of `slakio_core::backend`: the app
  queues commands, the binary's loop delivers them and hands the events back
  (`exchange.rs`, until neither side has more: an answer may ask for the next page), and an
  event for an older request (an older generation) or for a target no pane shows is dropped.
  Messages come in pages (`Command::History`), newest first, older ones as the selection nears
  the top.
- What only the binary can do is an `Effect` the loop carries out: copying is OSC 52, so no
  clipboard library or helper process is involved; saving an answer the app asked for (the
  icons question) goes into the config file, keeping its comments. `slakio --demo` uses `DemoBackend`
  (`crates/slakio-tui/src/demo.rs`) over `slakio-world`; `main.rs` is the one place that names
  a concrete backend.
- Channels from background tasks are bounded wherever a stream can be large (websocket events,
  history pages, downloads); events carry their target and a generation, and stale ones are
  dropped.

## Remote text

Everything a remote party chose — names of workspaces, people, sections, conversations and
reactions, message texts, later topics, statuses, file names and link texts — is
`slakio_core::sanitize::Remote`. It has no `Display`, `Deref` or `AsRef<str>`, so it reaches a
widget only through `line()` / `block()`, the sanitiser, which removes escape sequences
(7-bit and 8-bit), control, bidi and invisible characters, private-use code points, mark
floods and overlong text. `Remote::unsanitized` (comparisons and lookups) and `Safe::trusted`
are kept out of the drawing code by a test. Message texts are sanitised once, when their page
arrives; the composer sanitises what is typed or pasted into it.

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

## Keys

`crates/slakio-tui/src/keymap.rs` holds the context tree and the default bindings; every
binding names an action of the registry. A few global keys (`Ctrl+Q`, `F1`, `Ctrl+P`) are looked
up first, also while typing; then keys resolve from the focused context outwards; a text input
or a modal context (the help, a question) has no parent, so `Ctrl+W` can close a pane in Normal
mode and delete a word while typing. Sequences (`g g`, `Space w h`) wait for their next key;
after a short wait the which-key popup lists what may follow (`keymap/guide.rs`), its delay one
more deadline of the loop. The checker (`keymap/check.rs`) runs on the defaults in the tests and
rejects duplicates, shadowed parent keys, prefixes, character keys in text inputs, keys a
terminal cannot tell apart without the kitty keyboard protocol (`Ctrl+I`/`Tab`,
`Ctrl+M`/`Enter`, `Ctrl+[`/`Esc`, `Ctrl+H`/`Backspace`) bound to different actions, actions
reachable only by such keys or `Alt` keys, and a context taking a global key. `Ctrl+I` is bound
only when the terminal has the protocol on. The hint line (`keymap/hints.rs`), the keyboard help,
the empty states, the command palette and `docs/keybindings.md` all show keys looked up in the
key map, so a key is shown as bound.

The command palette (`app/palette.rs`, drawn by `ui/palette.rs`) is the `:` command line with a
list: `Ctrl+P` and `:` open it, and its entries are generated from the action registry, each
with its keys from the context under the palette. A typed word ranks them (`action::rank`): a
command it names, then one it starts, then an entry with a word it starts, then one with its
letters in order from a word start; `Quit` sorts after everything that matches as well, so it is
never the first entry of the full list. `:theme ` lists the themes, `:avatars ` its values.
Its geometry (`screen::palette`) is shared by drawing and the mouse.

## The look

`crates/slakio-tui/src/theme.rs` holds the color tokens and the styles made of them; widgets
name roles, never colors. The built-ins are the terminal's own 16 colors, `dark`, `light`,
`high-contrast`, `nord`, `dracula` and three families (`catppuccin`, `tokyo-night`, `gruvbox`)
whose light or dark variant follows the terminal's background, asked once with OSC 11; `auto`
takes `tokyo-night` on a terminal that says it shows 24-bit color. `:theme <name>` resolves a
name again with what was found at startup (`theme::Look`) and saves it to the config file. Every
truecolor theme passes WCAG contrast checks (`theme/tests.rs`): body text 4.5:1 on every surface,
muted text and marks 3:1, pills, mode badges and the initials on avatar chips 4.5:1 (4.3:1 on
a dark theme's tinted chip). An avatar chip (`avatar.rs`: initials and a color slot hashed
from the person's id) takes one of the theme's avatar hues, none in the red family (red is the
mention pills'); a truecolor theme draws it as a tint of the hue on the background with the
hue as the initials (darkened on a light theme), never brighter than the selection bar or the
accent, and the 16-color theme as colored initials without a background: a solid block is a
pill's. Two rules hold in every theme and are
tested cell by cell (`tests/style_flows.rs`): the focus shows on a panel's border and title
only, and a selection is a background (or a bar in the left gutter), never an underline.

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
