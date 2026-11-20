//! Whether pictures are drawn. Never garbled: unless slakio is sure the terminal draws the kitty
//! graphics protocol's placeholder cells, it draws the text cards. It tells from the
//! environment alone ([`Env`], a snapshot of it), never by asking the terminal (the answer would
//! come on standard input, behind the keys' reader).
//!
//! * kitty (`TERM=xterm-kitty`, with or without `KITTY_WINDOW_ID`) and Ghostty
//!   (`TERM=xterm-ghostty`, or `TERM_PROGRAM=ghostty` with `TERM` `xterm-256color` or `xterm`,
//!   what Ghostty sets where its own entry is missing) draw them; nothing else counts. A variable
//!   of theirs alone says nothing: a program inside them that is a terminal of its own (a
//!   multiplexer, Neovim's or Emacs' terminal) passes them on (`GHOSTTY_RESOURCES_DIR`,
//!   `KITTY_WINDOW_ID` with another `TERM`).
//! * Inside a multiplexer or another terminal in a terminal (tmux, screen, Zellij, herdr, dvtm,
//!   Neovim, Emacs: [`MULTIPLEXER_VARS`], [`MULTIPLEXER_TERMS`], [`MULTIPLEXER_PROGRAMS`]) the
//!   escapes would reach it, not the terminal: text cards, whatever terminal is outside.
//! * The setting `images` (`auto`, the default; `on`; `off`) and [`IMAGES_ENV`] (the same
//!   values; it wins over the setting) override that: `on` draws pictures even where slakio is
//!   not sure (the user's choice), `off` never does. A value of the variable not understood is
//!   `off`. The demo (`--demo`) draws pictures only when `on` is asked for.

/// The environment variable that overrides the `images` setting: `auto`, `on` or `off`.
pub const IMAGES_ENV: &str = "SLAKIO_IMAGES";

/// The variables a multiplexer (or an editor's terminal) sets in the shells it runs.
pub const MULTIPLEXER_VARS: &[&str] =
    &["TMUX", "STY", "ZELLIJ", "ZELLIJ_SESSION_NAME", "HERDR_ENV", "NVIM", "INSIDE_EMACS"];
/// The starts of the `TERM` values a multiplexer sets.
pub const MULTIPLEXER_TERMS: &[&str] = &["screen", "tmux", "dvtm"];
/// The `TERM` values Ghostty runs a shell with (its own, or the common ones where its own entry
/// is missing).
const GHOSTTY_TERMS: &[&str] = &["xterm-ghostty", "xterm-256color", "xterm"];
/// The `TERM_PROGRAM` values a multiplexer sets.
pub const MULTIPLEXER_PROGRAMS: &[&str] = &["tmux", "screen", "zellij", "herdr"];

/// What decides it, read once from the environment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env {
    pub term: String,
    pub term_program: String,
    pub kitty_window: bool,
    /// One of [`MULTIPLEXER_VARS`] is set (not empty).
    pub multiplexer_var: bool,
    /// [`IMAGES_ENV`], when set.
    pub images: Option<String>,
}

impl Env {
    /// The snapshot of the environment `env` reads.
    pub fn read(env: impl Fn(&str) -> Option<String>) -> Self {
        let set = |k: &str| env(k).is_some_and(|v| !v.is_empty());
        Self {
            term: env("TERM").unwrap_or_default(),
            term_program: env("TERM_PROGRAM").unwrap_or_default(),
            kitty_window: set("KITTY_WINDOW_ID"),
            multiplexer_var: MULTIPLEXER_VARS.iter().any(|k| set(k)),
            images: env(IMAGES_ENV),
        }
    }

    /// Inside a multiplexer, as far as the environment tells.
    pub fn multiplexed(&self) -> bool {
        let program = self.term_program.to_ascii_lowercase();
        self.multiplexer_var
            || MULTIPLEXER_TERMS.iter().any(|t| self.term.starts_with(t))
            || MULTIPLEXER_PROGRAMS.contains(&program.as_str())
    }

    /// The terminal surely draws kitty graphics placeholders: kitty or Ghostty, no multiplexer.
    pub fn terminal_draws(&self) -> bool {
        let kitty = self.term == "xterm-kitty";
        let ghostty = self.term == "xterm-ghostty"
            || (self.term_program.eq_ignore_ascii_case("ghostty") && GHOSTTY_TERMS.contains(&self.term.as_str()));
        (kitty || ghostty) && !self.multiplexed()
    }
}

/// Pictures are drawn, by the setting `config` (`auto`, `on`, `off`), [`IMAGES_ENV`] (which
/// wins) and the terminal; `demo`: only when `on` is asked for.
pub fn images_on(env: &Env, config: &str, demo: bool) -> bool {
    let choice = match env.images.as_deref().map(str::trim) {
        None | Some("") => config,
        Some(v) => v,
    };
    match choice {
        "on" => true,
        "auto" => !demo && env.terminal_draws(),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
