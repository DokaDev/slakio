//! slakio terminal UI (Ratatui). The library holds all state and rendering so it can be tested
//! headless; the `slakio` binary (`main.rs`) only owns the terminal and the event loop.
//!
//! * [`app`] — application state: a thin router over sub-states that own their update.
//! * [`action`] — the action registry (ids, labels, `:` commands).
//! * [`keymap`] — key contexts, default bindings, key notation, `docs/keybindings.md`.
//! * [`ui`] — drawing a frame.
//! * [`theme`] — color and style tokens.
//! * [`terminal`] — the terminal modes the binary sets and restores, and the cursor's shape.
//! * [`kitty`] — the kitty keyboard protocol flags.
//!
//! UI-independent logic lives in `slakio-core`.

pub mod action;
pub mod app;
pub mod keymap;
pub mod kitty;
pub mod terminal;
pub mod theme;
pub mod ui;
