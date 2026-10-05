//! The theme: the color and style tokens every widget draws with. Widgets never name a color
//! of their own.
//!
//! The default theme, [`Theme::terminal`], uses only the terminal's own 16 ANSI colors, so it
//! follows the user's terminal palette. With `NO_COLOR` set (to anything but the empty string,
//! <https://no-color.org>) [`Theme::no_color`] draws without any color: everything a color
//! marks also has a shape (a text badge, a letter, a dot, reversed or bold text), so nothing is
//! told by color alone. User themes, truecolor built-ins and the light/dark families come later
//! and add tokens here.

use ratatui::style::{Color, Modifier, Style};
use slakio_core::model::WorkspaceColor;

/// The tokens of a theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Ordinary text.
    pub text: Style,
    /// Secondary text: hints, explanations.
    pub muted: Style,
    /// The product name on the empty work area.
    pub title: Style,
    /// The status line behind its segments.
    pub status: Style,
    /// The mode badges at the left of the status line, one per mode. Each has its text as
    /// well, so the mode never depends on the color.
    pub mode_normal: Style,
    pub mode_command: Style,
    /// A transient notice that warns (an unknown command, a config file that cannot be used).
    pub warning: Style,
    /// The border of the region that has the focus, and of the others.
    pub border_focus: Style,
    pub border: Style,
    /// The row under the cursor of the focused region, and of a region without the focus.
    pub cursor: Style,
    pub cursor_inactive: Style,
    /// A section header of the list panel.
    pub section: Style,
    /// A conversation with unread messages (its name; a `●` marks it too).
    pub unread: Style,
    /// The mention count badge.
    pub mention: Style,
    /// A muted conversation.
    pub muted_conversation: Style,
    /// The current workspace or view on the rail.
    pub current: Style,
    /// The backend's state in the status line (`demo`).
    pub connection: Style,
    /// Workspace colours, by [`WorkspaceColor`] slot. They belong to the workspace, so they
    /// stay the same in every theme that has colours; a letter names the workspace too.
    pub workspaces: [Style; 4],
}

impl Theme {
    /// The default: the terminal's own colors.
    pub fn terminal() -> Self {
        let badge = |bg: Color| Style::new().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD);
        Self {
            text: Style::new(),
            muted: Style::new().fg(Color::DarkGray),
            title: Style::new().add_modifier(Modifier::BOLD),
            status: Style::new(),
            mode_normal: badge(Color::Blue),
            mode_command: badge(Color::Yellow),
            warning: Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            border_focus: Style::new().fg(Color::Blue),
            border: Style::new().fg(Color::DarkGray),
            cursor: Style::new().add_modifier(Modifier::REVERSED),
            cursor_inactive: Style::new().add_modifier(Modifier::UNDERLINED),
            section: Style::new().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
            unread: Style::new().add_modifier(Modifier::BOLD),
            mention: Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            muted_conversation: Style::new().fg(Color::DarkGray),
            current: Style::new().add_modifier(Modifier::BOLD),
            connection: Style::new().fg(Color::Yellow),
            workspaces: [
                Style::new().fg(Color::Green),
                Style::new().fg(Color::Magenta),
                Style::new().fg(Color::Cyan),
                Style::new().fg(Color::Yellow),
            ],
        }
    }

    /// No color at all (`NO_COLOR`): shapes only.
    pub fn no_color() -> Self {
        let badge = Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        let bold = Style::new().add_modifier(Modifier::BOLD);
        Self {
            text: Style::new(),
            muted: Style::new(),
            title: bold,
            status: Style::new(),
            mode_normal: badge,
            mode_command: badge,
            warning: bold,
            border_focus: bold,
            border: Style::new(),
            cursor: Style::new().add_modifier(Modifier::REVERSED),
            cursor_inactive: Style::new().add_modifier(Modifier::UNDERLINED),
            section: bold,
            unread: bold,
            mention: bold,
            muted_conversation: Style::new().add_modifier(Modifier::DIM),
            current: bold,
            connection: Style::new(),
            workspaces: [Style::new(); 4],
        }
    }

    /// The theme for the environment `env`: [`Theme::no_color`] when `NO_COLOR` is set and
    /// not empty, else the default.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Self {
        if env("NO_COLOR").is_some_and(|v| !v.is_empty()) { Self::no_color() } else { Self::terminal() }
    }

    /// The style of workspace colour `c`.
    pub fn workspace(&self, c: WorkspaceColor) -> Style {
        self.workspaces[usize::from(c.0) % self.workspaces.len()]
    }
}

#[cfg(test)]
mod tests;
