# slakio

An unofficial terminal client for Slack, written in Rust with [Ratatui](https://ratatui.rs).

> **Unofficial.** slakio is an independent open-source project. It is not affiliated with,
> endorsed by, or sponsored by Slack Technologies, LLC or Salesforce. "Slack" is a trademark of
> its owner and is used here only to say which service the client is meant to work with.

## Status

**Pre-alpha, early development. slakio does not connect to Slack yet.**

What exists today is the project's foundation and the first part of the interface, on
invented data only (`slakio --demo`): the rail, the list panel of channels and DMs, the work
area and the status line. There is no sign-in, no real workspace and no message history yet,
and there is no release to install. Nothing here is ready for daily use.

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
    version) expands when focused or hovered;
  - the list panel shows sections (fold with `Enter`), channels and DMs with unread dots and
    mention counts; `Enter` or a click opens a conversation's frame in the work area (its
    messages are not shown yet);
  - `Ctrl+h` / `Ctrl+l` (or `Space w h` / `Space w l`) move between the rail, the list and the
    work area; `Space h` / `Space d` show Home / DMs; `Space e` hides the list panel;
  - every key is listed in [docs/keybindings.md](docs/keybindings.md).
- `:qa` then `Enter` quits (`:q` too). `Esc` closes the command line.
- English and Korean interface text (`language = "auto" | "en" | "ko"` in the config file).
- `NO_COLOR=1` draws without colors.

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
icons = false            # Nerd Font icons on the rail and the status line (else letters)
rail_expand = "overlay"  # the focused rail opens over the list panel, or "push"es it aside
```

`rail_expand` is temporary: both ways exist until one is chosen.

A config file that cannot be used is never overwritten: slakio starts with the defaults and
says why in the status line.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the gates every change passes, and
[docs/architecture.md](docs/architecture.md) for how the code is laid out.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
