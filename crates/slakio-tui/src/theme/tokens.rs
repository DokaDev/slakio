//! The `[styles]` of a theme file: overrides of roles the theme otherwise computes from its
//! colours. A role checks its override first ([`over`]) and falls back to its own formula, so a
//! built-in theme (no override, [`Styles::NONE`]) draws exactly as it always has.

use super::{Selection, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

/// The style tokens a theme file may set, each `None` where the computed role stays. The names
/// are [`slakio_core::theme_file::STYLE_TOKENS`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Styles {
    /// The selected row of the panel with the focus: its `bg` replaces the bar's colour, its
    /// `fg` and modifiers go on the text of the row, never on a picture's cells.
    pub selection: Option<Style>,
    /// The selected row of a panel without the focus, as `selection`.
    pub cursor_line: Option<Style>,
    pub link: Option<Style>,
    pub mention: Option<Style>,
    pub badge: Option<Style>,
    pub current: Option<Style>,
    pub code: Option<Style>,
    pub reaction: Option<Style>,
    pub reaction_mine: Option<Style>,
    pub tab_active: Option<Style>,
    pub tab_badge: Option<Style>,
    pub status_line: Option<Style>,
    pub hint: Option<Style>,
    pub search_match: Option<Style>,
}

impl Styles {
    /// No override: every role as the theme computes it.
    pub const NONE: Styles = Styles {
        selection: None,
        cursor_line: None,
        link: None,
        mention: None,
        badge: None,
        current: None,
        code: None,
        reaction: None,
        reaction_mine: None,
        tab_active: None,
        tab_badge: None,
        status_line: None,
        hint: None,
        search_match: None,
    };

    /// The slot of style token `name` (`None`: not a style token).
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<Style>> {
        Some(match name {
            "selection" => &mut self.selection,
            "cursor_line" => &mut self.cursor_line,
            "link" => &mut self.link,
            "mention" => &mut self.mention,
            "badge" => &mut self.badge,
            "current" => &mut self.current,
            "code" => &mut self.code,
            "reaction" => &mut self.reaction,
            "reaction_mine" => &mut self.reaction_mine,
            "tab_active" => &mut self.tab_active,
            "tab_badge" => &mut self.tab_badge,
            "status_line" => &mut self.status_line,
            "hint" => &mut self.hint,
            "search_match" => &mut self.search_match,
            _ => return None,
        })
    }
}

/// The override `role` when a theme file sets one, else the computed `base`.
pub(super) fn over(role: Option<Style>, base: impl FnOnce() -> Style) -> Style {
    role.unwrap_or_else(base)
}

impl Theme {
    /// The count pill of mentions and unread DMs.
    pub fn badge(&self) -> Style {
        over(self.styles.badge, || self.badge_role())
    }

    /// The current workspace or view.
    pub fn current(&self) -> Style {
        over(self.styles.current, || self.current_role())
    }

    /// The "N replies" row of a message with a thread, and a link in a message.
    pub fn link(&self) -> Style {
        over(self.styles.link, || self.link_role())
    }

    /// A person, channel or group named in a message: bold, in the accent; the user named (or
    /// everyone, `@here`) in the warm color, as GUI Slack marks a mention of the user. A theme
    /// file's `mention` replaces both.
    pub fn mention(&self, of_me: bool) -> Style {
        over(self.styles.mention, || self.mention_role(of_me))
    }

    /// Code in a message (inline, or a code block's lines).
    pub fn code(&self) -> Style {
        over(self.styles.code, || self.code_role())
    }

    /// The letters that pick a link (`o` on a message with several).
    pub fn hint(&self) -> Style {
        over(self.styles.hint, || self.hint_role())
    }

    /// A reaction chip: its pad and emoji ([`Self::reaction_chip`]).
    pub fn reaction(&self) -> Style {
        self.reaction_chip(false).body
    }

    /// The user's own reaction: its pad and emoji ([`Self::reaction_chip`]).
    pub fn reaction_mine(&self) -> Style {
        self.reaction_chip(true).body
    }

    /// The status line: its surface.
    pub fn status_line(&self) -> Style {
        over(self.styles.status_line, || self.surface())
    }

    /// A word a search found, in a result's text: bold and underlined in the warm color.
    pub fn search_match(&self) -> Style {
        over(self.styles.search_match, || self.key_warm().add_modifier(Modifier::UNDERLINED))
    }

    /// The override of a selection drawn `how` (`selection`, `cursor_line`; none for a VISUAL
    /// range, whose colour is `range`).
    pub(super) fn selection_style(&self, how: Selection) -> Option<Style> {
        match how {
            Selection::Focused => self.styles.selection,
            Selection::Unfocused => self.styles.cursor_line,
            Selection::Visual => None,
        }
    }

    /// The `fg` and modifiers of the selection override of `how` on the row `row` (already
    /// painted): the cells the bar keeps as they are (a pill, an avatar chip, a picture's cell,
    /// whose colour says which picture it shows) are never touched.
    pub(super) fn paint_over(&self, buf: &mut Buffer, row: Rect, how: Selection) {
        let Some(style) = self.selection_style(how).filter(|s| s.fg.is_some() || !s.add_modifier.is_empty()) else {
            return;
        };
        for x in row.left()..row.right() {
            if self.keeps(&buf[(x, row.y)]) {
                continue;
            }
            let cell = &mut buf[(x, row.y)];
            if let Some(fg) = style.fg {
                cell.fg = fg;
            }
            cell.modifier.insert(style.add_modifier);
        }
    }
}
