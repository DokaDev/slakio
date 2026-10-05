# slakio

An unofficial terminal client for Slack, written in Rust with [Ratatui](https://ratatui.rs).

> **Unofficial.** slakio is an independent open-source project. It is not affiliated with,
> endorsed by, or sponsored by Slack Technologies, LLC or Salesforce. "Slack" is a trademark of
> its owner and is used here only to say which service the client is meant to work with.

## Status

**Pre-alpha, early development. slakio does not connect to Slack yet.**

What exists today is the project's foundation: the workspace layout, the build and test
gates, and a binary that opens an empty screen with a status line and quits cleanly. There is
no sign-in, no workspace, no channel and no message yet, and there is no release to install.
Nothing here is ready for daily use.

The plan, in order: a user interface on fake data first, then read-only Slack connectivity,
then sending messages, drafts, the Activity view and search, reactions and emoji, files and
images, and settings. This README only lists what works; it will grow with each step.

## What works

- `slakio --version` and `slakio --help`.
- `slakio` opens a full-screen, empty work area with a status line (mode badge) and restores
  the terminal on exit, on `SIGTERM`/`SIGHUP`, and on a crash.
- `:qa` then `Enter` quits (`:q` too). `Esc` closes the command line.
- English and Korean interface text (`language = "auto" | "en" | "ko"` in the config file).
- `NO_COLOR=1` draws without colors.

## Build from source

Requires Rust 1.93 (pinned in `rust-toolchain.toml`; rustup installs it automatically).

```sh
git clone https://github.com/DokaDev/slakio
cd slakio
cargo run -p slakio-tui
```

macOS is the primary platform. Linux and Windows are built and tested in CI but nobody has used
slakio on them yet.

## Configuration

`$XDG_CONFIG_HOME/slakio/config.toml`, else `~/.config/slakio/config.toml` (or `--config
<path>`):

```toml
language = "auto"   # "auto" (from LC_ALL / LC_MESSAGES / LANG), "en" or "ko"
```

A config file that cannot be used is never overwritten: slakio starts with the defaults and
says why in the status line.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the gates every change passes, and
[docs/architecture.md](docs/architecture.md) for how the code is laid out.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
