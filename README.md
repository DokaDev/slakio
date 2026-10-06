# slakio

An unofficial terminal client for Slack, written in Rust with [Ratatui](https://ratatui.rs).

> **Unofficial.** slakio is an independent open-source project. It is not affiliated with,
> endorsed by, or sponsored by Slack Technologies, LLC or Salesforce. "Slack" is a trademark of
> its owner and is used here only to say which service the client is meant to work with.

## Status

**Pre-alpha, early development. slakio does not connect to Slack yet.**

What exists today is the project's foundation and the first part of the interface, on
invented data only (`slakio --demo`): the rail, the list panel of channels and DMs, the
conversation pane with a thread panel and a composer, and the status line. There is no sign-in
and no real workspace yet; nothing is ever sent anywhere, and there is no release to install.
Nothing here is ready for daily use.

The plan, in order: a user interface on fake data first, then read-only Slack connectivity,
then sending messages, drafts, the Activity view and search, reactions and emoji, files and
images, and settings. This README only lists what works; it will grow with each step.

## What works

- `slakio --version` and `slakio --help`.
- `slakio` opens a full-screen, empty work area with a status line (mode badge) and restores
  the terminal on exit, on `SIGTERM`/`SIGHUP`, and on a crash.
- `slakio --demo` shows an invented world (two workspaces, sidebar sections, about 300
  channels, DMs, a channel shared with another organization), nothing of which connects
  anywhere:
  - the rail (workspaces, Home, DMs; Activity, Files and Later say they come in a later
    version) expands when focused or hovered; `Ctrl+R` or `Space r` goes there from anywhere
    (and `Ctrl+R` back), `j` / `k` and `Enter` pick a workspace or a view, `Esc` leaves;
  - each DM shows whether its person is active (`●`), away (`○`) or in do not disturb (`◐`),
    in the list and in the conversation's title;
  - the list panel shows sections (fold with `Enter`), channels and DMs, unread ones in bold
    with mention counts; `Enter` or a click opens the conversation in the work area and moves
    there, `l` opens it and stays in the list;
  - the conversation shows its messages (date separators, edited marker, reaction pills,
    "N replies" rows), loading older ones as you go up; `j` / `k` select messages, `g g` /
    `G` jump to the oldest / newest (arrows, `PageUp` / `PageDown`, `Home` / `End` and the mouse
    wheel too), `Enter` opens the selected message's thread in a panel on the right (another
    thread replaces it; with no message selected it writes), `Ctrl+W` closes the panel, then
    the conversation; `Ctrl+O` / `Space [` and `Space ]` go back and forward between the
    conversations a pane showed;
  - tabs: `t` opens the conversation under the list's cursor, or the selected message's thread,
    in a new tab (what is open already, in any tab, is focused instead, also by `Enter`); the
    tab bar shows from two tabs on, with the unread count of the tabs not shown; `g t` / `g T`,
    `Ctrl+PageDown` / `Ctrl+PageUp`, `Space 1`…`9` (`Alt+1`…`9`) or a click switch tabs;
    `Ctrl+W` on a tab's last pane, `Space t c`, its `×` or a middle click close a tab (the last
    one leaves an empty work area, nothing quits) and `Space t u` opens the tabs closed this
    session again where they were; `Space t r`, `:rename <name>` or a double click rename one;
    `Space t h` / `Space t l` or a drag move one. The app never reorders tabs. Tabs and their
    names are not kept across restarts yet;
  - `V` selects a range of messages and `y` copies it (or the selected message) to the
    clipboard through the terminal (OSC 52);
  - `i` writes in the pane's composer (multiline: `Ctrl+J` or `Alt+Enter` for a new line,
    `Shift+Enter` with the kitty keyboard protocol; `Ctrl+W` deletes a word; Korean input
    works); `Enter` shows the message in the demo only, it is not sent anywhere; `Esc` goes
    back to Normal mode;
  - `Esc` steps out one level at a time (VISUAL, the selection, the thread panel, the main
    pane) and never closes anything;
  - with a Korean input source, Normal-mode keys still work (`j` typed as its jamo moves);
  - every name and message from the (invented) remote side is shown through a sanitiser that
    removes terminal escape sequences, control and bidi characters; the demo has a channel of
    hostile strings and a few hostile names to show it;
  - `Tab` / `Shift+Tab` go round the rail, the list, the main pane and the thread panel; `Ctrl+h` /
    `Ctrl+l` (or `Space w h` / `Space w l`) move left and right, the rail included; `Space h` /
    `Space d` show Home / DMs; `Space e` hides the list panel;
  - the status line shows the keys worth knowing where you are, and every key is listed in
    [docs/keybindings.md](docs/keybindings.md).
- `Ctrl+Q` quits from anywhere (it asks first when a message you wrote was not sent); so do
  `Space q` and `:q` / `:qa`.
- `Ctrl+P` (or `:`) opens the command palette: every command and action that works where you
  are, with its keys; type to filter (letters in order are enough: `thm` finds `:theme`),
  `Tab` / arrows to select, `Enter` to run, or click (a click on Quit asks first).
- People are pictured by their initials on a color of their own: before a sender's name, before
  a DM in the list (with whether they are around at its corner) and in a DM's title;
  `:avatars` turns them off and on and saves it. Profile photos are not shown yet.
- Themes: the terminal's own colors, `dark`, `light`, `high-contrast`, `nord`, `dracula`, and
  `catppuccin`, `tokyo-night` and `gruvbox` (each its light or dark variant by the terminal's
  background); `:theme <name>` switches while running and saves it; `NO_COLOR=1` draws without
  colors.

## Keys

Six keys get you everywhere:

| Key | What it does |
|---|---|
| `?` (or `F1`) | The keyboard help for where you are: every key, searchable, and `Enter` runs one |
| `Space` | Wait a moment: a popup lists what may follow (`Space h` Home, `Space d` DMs, …) |
| `Tab` | The next panel, the rail included (`Shift+Tab` the previous one) |
| `Ctrl+P` | The command palette: every command and action by name, with its keys |
| `Esc` | One step out; never closes anything |
| `Ctrl+Q` | Quit |
- English and Korean interface text (`language = "auto" | "en" | "ko"` in the config file).

## Build from source

Requires Rust 1.93 (pinned in `rust-toolchain.toml`; rustup installs it automatically).

```sh
git clone https://github.com/DokaDev/slakio
cd slakio
cargo run -p slakio-tui -- --demo
```

macOS is the primary platform. Linux and Windows are built and tested in CI but nobody has used
slakio on them yet.

## Configuration

`$XDG_CONFIG_HOME/slakio/config.toml`, else `~/.config/slakio/config.toml` (or `--config
<path>`):

```toml
language = "auto"        # "auto" (from LC_ALL / LC_MESSAGES / LANG), "en" or "ko"
theme = "auto"           # "auto", "terminal", "dark", "light", "high-contrast", "nord",
                         # "dracula", or a family: "catppuccin" (-latte / -mocha),
                         # "tokyo-night" (-day / -night), "gruvbox" (-light / -dark)
icons = "ask"            # Nerd Font icons: "on", "off", or "ask" once (true / false work too)
avatars = "initials"     # a person's initials on a colored chip, or "off"
rail_expand = "overlay"  # the focused rail opens over the list panel, or "push"es it aside
```

`theme = "auto"` takes `tokyo-night` when the terminal says it shows 24-bit color (`COLORTERM`
is `truecolor` or `24bit`), else the terminal's own colors. `:theme <name>` (also
`:colorscheme`, or `:set theme=<name>`) switches the theme while slakio runs and saves it here,
keeping your comments. `:avatars` (or `:set avatars=off`) does the same for `avatars`;
`avatars = "image"` is kept for profile photos, which come in a later version, and draws
initials until then. With `icons = "ask"`, `slakio
--demo` asks once whether your font shows the icons (the rail previews the answer) and saves
the answer in the config file. `rail_expand` is temporary: both ways exist until one is chosen.

A config file that cannot be used is never overwritten: slakio starts with the defaults and
says why in the status line.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the gates every change passes, and
[docs/architecture.md](docs/architecture.md) for how the code is laid out.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
