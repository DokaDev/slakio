//! UI-free and network-free core of slakio: what the terminal UI and the adapters (Slack
//! client, authentication, local store) share.
//!
//! * [`paths`] — the config, data, state and cache directories; [`fsutil`] — atomic file writes.
//! * [`config`] — the config file.
//! * [`secret`] — the secret store interface, its in-memory implementation and the
//!   `SLAKIO_SECRET_STORE` switch.
//! * [`fault`] — failures as data (a kind the UI words, a detail for the error log).
//! * [`i18n`] — the embedded en/ko message catalogs.
//!
//! This crate must never depend on a UI crate (ratatui/crossterm), a network or database crate,
//! or another slakio crate.

pub mod config;
pub mod fault;
pub mod fsutil;
pub mod i18n;
pub mod paths;
pub mod secret;
