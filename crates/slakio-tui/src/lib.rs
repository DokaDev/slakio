//! slakio terminal UI (Ratatui). The library holds all state and rendering so it can be tested
//! headless; the `slakio` binary (`main.rs`) only owns the terminal and the event loop.
//!
//! * [`app`] — application state: a thin router over sub-states that own their update.
//! * [`action`] — the action registry (ids, labels, `:` commands).
//! * [`keymap`] — key contexts, default bindings, key notation, `docs/keybindings.md`.
//! * [`screen`] — the geometry of the main screen, shared by drawing and the mouse.
//! * [`ui`] — drawing a frame.
//! * [`demo`] — the demo backend over the invented world (`slakio --demo`).
//! * [`theme`] — color and style tokens.
//! * [`terminal`] — the terminal modes the binary sets and restores, and the cursor's shape.
//! * [`kitty`] — the kitty keyboard protocol flags.
//!
//! UI-independent logic lives in `slakio-core`.

pub mod action;
pub mod app;
pub mod demo;
pub mod keymap;
pub mod kitty;
pub mod screen;
pub mod terminal;
pub mod theme;
pub mod ui;
