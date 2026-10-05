//! What the binary changes on the terminal and undoes: the alternate
//! screen, mouse capture, bracketed paste, the kitty keyboard flags and the cursor's shape.
//! The escape sequences are written to any writer, so tests check them without a terminal.
//!
//! The cursor takes the shape of the mode ([`cursor_shape`]): a steady block in Normal mode, a
//! steady bar in every text input (the command line, later the composer). Whatever happens, the
//! terminal gets its user's default shape back: [`TermState::restore`] runs on a normal exit,
//! when an error ends the program, when setting the terminal up fails part way, and from the
//! panic hook ([`Guard`] and the binary's hook share one [`TermState`], and only the first call
//! writes). Handing the terminal over for a while (an external editor, a suspend with
//! `Ctrl+Z`) is a `restore` followed by raw mode and [`TermState::enter`] again: the state is
//! marked again, so whatever happens after it is undone once more.

use crate::app::App;
use crate::kitty;
use ratatui::crossterm::cursor::{SetCursorStyle, Show};
use ratatui::crossterm::event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture};
use ratatui::crossterm::queue;
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// A shape of the terminal's cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    /// A steady block: Normal mode.
    Block,
    /// A steady bar: every text input.
    Bar,
}

impl CursorShape {
    fn style(self) -> SetCursorStyle {
        match self {
            CursorShape::Block => SetCursorStyle::SteadyBlock,
            CursorShape::Bar => SetCursorStyle::SteadyBar,
        }
    }
}

/// The cursor shape for what has the keyboard now: a bar in a text input, a block otherwise
/// (the cursor is hidden there).
pub fn cursor_shape(app: &App) -> Option<CursorShape> {
    Some(if app.key_context().is_text_input() { CursorShape::Bar } else { CursorShape::Block })
}

/// The cursor shape last written to the terminal, so only a change is written.
#[derive(Debug, Default)]
pub struct Cursor {
    /// `None`: the terminal's own shape (never changed, or given back).
    set: Option<CursorShape>,
}

impl Cursor {
    /// Write what gives the cursor shape `want` (`None`: the user's default shape back, once
    /// a shape was set) to `out`, if it is not the shape already set.
    pub fn apply(&mut self, out: &mut impl Write, want: Option<CursorShape>) -> io::Result<()> {
        if self.set == want {
            return Ok(());
        }
        match want {
            Some(shape) => queue!(out, shape.style())?,
            None => queue!(out, SetCursorStyle::DefaultUserShape)?,
        }
        out.flush()?;
        self.set = want;
        Ok(())
    }
}

/// What the binary turned on, so it is undone exactly once however the program ends.
#[derive(Debug, Default)]
pub struct TermState {
    /// Raw mode is on (it is not an escape sequence: [`TermState::restore`] calls back).
    raw: AtomicBool,
    /// The alternate screen, mouse and paste modes were (or were being) turned on.
    entered: AtomicBool,
    /// The kitty keyboard flags were pushed and must be popped.
    keys: AtomicBool,
}

impl TermState {
    pub const fn new() -> Self {
        Self { raw: AtomicBool::new(false), entered: AtomicBool::new(false), keys: AtomicBool::new(false) }
    }

    /// Raw mode was turned on.
    pub fn raw_on(&self) {
        self.raw.store(true, Ordering::SeqCst);
    }

    /// Enter the alternate screen and turn on mouse capture and bracketed paste, and with
    /// `enhanced` push the kitty keyboard flags (the main and alternate screens keep separate
    /// flag stacks: pushed after entering, popped before leaving). Marked first, so a failure
    /// part way is undone too.
    pub fn enter(&self, out: &mut impl Write, enhanced: bool) -> io::Result<()> {
        self.entered.store(true, Ordering::SeqCst);
        queue!(out, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
        out.flush()?;
        if enhanced {
            self.keys.store(true, Ordering::SeqCst);
            out.write_all(kitty::push_sequence().as_bytes())?;
            out.flush()?;
        }
        Ok(())
    }

    /// Undo what was turned on, once (later calls do nothing until it is turned on again):
    /// `raw_off` for raw mode, then on `out` the kitty flags popped, bracketed paste and mouse
    /// capture off, the cursor shown in the user's default shape and the main screen. Every step is tried
    /// even when one fails; the first failure comes back.
    pub fn restore(&self, out: &mut impl Write, raw_off: impl FnOnce()) -> io::Result<()> {
        if self.raw.swap(false, Ordering::SeqCst) {
            raw_off();
        }
        if !self.entered.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        let popped = match self.keys.swap(false, Ordering::SeqCst) {
            true => out.write_all(kitty::pop_sequence().as_bytes()),
            false => Ok(()),
        };
        let rest = queue!(
            out,
            DisableBracketedPaste,
            DisableMouseCapture,
            Show,
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        let flushed = out.flush();
        popped.and(rest).and(flushed)
    }
}

/// Restores a [`TermState`] when dropped: a normal return, an error returned with `?`, or a
/// panic unwinding through it. `out` gives the writer to restore on, `raw_off` turns raw mode
/// off.
pub struct Guard<'a, W: Write, O: FnMut() -> W> {
    state: &'a TermState,
    out: O,
    raw_off: fn(),
}

impl<'a, W: Write, O: FnMut() -> W> Guard<'a, W, O> {
    pub fn new(state: &'a TermState, out: O, raw_off: fn()) -> Self {
        Self { state, out, raw_off }
    }
}

impl<W: Write, O: FnMut() -> W> Drop for Guard<'_, W, O> {
    fn drop(&mut self) {
        let _ = self.state.restore(&mut (self.out)(), self.raw_off);
    }
}

/// The OSC 52 sequence that asks the terminal to put `text` on the system clipboard. The
/// terminal does it (Ghostty, kitty, iTerm2, WezTerm, tmux with `set-clipboard on`); no
/// clipboard library or helper process is involved. `text` is the sanitised text of messages.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
