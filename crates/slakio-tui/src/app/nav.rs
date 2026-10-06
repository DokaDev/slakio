//! Navigation in the list panel as the app routes it: what the workspace chip (the panel's
//! title) and the view switcher (its first row) say, the workspace switcher the chip opens, and
//! the mouse on both. The geometry is [`crate::navbar`]'s.
//!
//! The keyboard reaches the view switcher with `Ctrl+R` / `Space r` (`:nav`), or `Tab` round
//! the panels; `h`/`l` and the arrows move along it, `Enter` shows the view under the cursor in
//! the list panel, `Esc` goes back. In the list, `[` / `]` show the view before or after.
//! `Space W` opens the workspace switcher from anywhere outside text; in it `j`/`k` move,
//! `Enter` switches, `Esc` closes.
//!
//! Counts (ui-ux-spec §H.1): Home `@n` the mentions in its channels, else `●` when a channel is
//! unread; DMs `●n` the unread messages of its DMs; Activity `@n` the mentions of every
//! workspace. Only mentions are `@` and red. Another workspace's mark follows `▾` with its letter.

use super::shell::View;
use super::tabs::Unread;
use super::{App, Focus};
use crate::action::ShellAction;
use crate::navbar::{self, Bar, Chip, Item, Part};
use crate::screen;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

impl App {
    /// What view `v` of workspace `ws` holds unread, counted as the list's pills count it.
    pub fn view_unread(&self, ws: usize, v: View) -> Unread {
        let Some(w) = self.model.workspaces().get(ws) else { return Unread::default() };
        let mut u = Unread::default();
        for c in self.model.conversations().iter().filter(|c| v == View::Activity || c.workspace == w.id) {
            match v {
                View::Home if !c.is_dm() => {
                    u.mentions += c.mentions;
                    u.any |= c.unread > 0 && !c.muted;
                }
                // A DM's unread messages, all of them (a DM is for the user anyway): `●n`.
                View::Dms if c.is_dm() => {
                    if c.unread > 0 && !c.muted {
                        u.any = true;
                        u.dms += c.unread;
                    }
                    u.dm_mention |= c.mentions > 0;
                }
                View::Activity => u.mentions += c.mentions,
                _ => {}
            }
        }
        u
    }

    /// The workspace chip as laid out now on the list panel's title (`None` without a backend
    /// or while the list panel is not shown).
    pub fn chip_bar(&self) -> Option<Bar> {
        self.backend?;
        let list = self.areas().list?;
        let ws = self.model.workspaces().get(self.shell.workspace)?;
        let others = (0..self.model.workspaces().len())
            .filter(|w| *w != self.shell.workspace)
            .filter_map(|w| {
                let u = Unread {
                    mentions: self.model.workspace_mentions(w),
                    any: self.model.workspace_unread(w),
                    ..Unread::default()
                };
                let letter = self.model.workspaces()[w].name.line().as_str().chars().next()?.to_uppercase().to_string();
                u.badge().map(|m| (letter, m))
            })
            .collect();
        let chip = Chip { name: ws.name.line().into_string(), others };
        Some(navbar::chip(screen::list_parts(list).title, &chip))
    }

    /// The view switcher as laid out now on the list panel's first row (`None` without a
    /// backend or while the list panel is not shown).
    pub fn view_switcher(&self) -> Option<Bar> {
        self.backend?;
        let list = self.areas().list?;
        let icons = self.settings.icons;
        let views: Vec<Item> = View::ALL
            .iter()
            .map(|v| Item {
                glyph: icons.then(|| v.glyph(true)),
                label: self.i18n.label(v.label()).to_string(),
                short: v.glyph(false).to_string(),
                badge: self.view_unread(self.shell.workspace, *v).badge(),
            })
            .collect();
        let shown = View::ALL.iter().position(|v| *v == self.shell.view).unwrap_or(0);
        Some(navbar::views(screen::list_parts(list).views, &views, shown))
    }

    /// Open the workspace switcher, its cursor on the workspace shown.
    pub(super) fn open_switcher(&mut self) {
        if self.backend.is_some() && !self.model.workspaces().is_empty() {
            self.help = None;
            self.switcher = Some(self.shell.workspace);
        }
    }

    /// The switcher's box on the screen now, while it is open.
    pub fn switcher_box(&self) -> Option<Rect> {
        self.switcher?;
        let x = self.chip_bar().and_then(|b| b.pieces.iter().find(|p| p.part == Part::Band).map(|p| p.x)).unwrap_or(0);
        Some(screen::switcher(self.size, x, self.model.workspaces().len()))
    }

    /// A key of the switcher.
    pub(super) fn switcher_key(&mut self, a: ShellAction) {
        let Some(at) = self.switcher else { return };
        let n = self.model.workspaces().len().max(1);
        match a {
            ShellAction::SwitcherNext => self.switcher = Some((at + 1) % n),
            ShellAction::SwitcherPrev => self.switcher = Some((at + n - 1) % n),
            ShellAction::SwitcherChoose => self.switch_to(at),
            _ => self.switcher = None,
        }
    }

    /// Show workspace `ws`; the list panel gets the keyboard.
    fn switch_to(&mut self, ws: usize) {
        self.switcher = None;
        self.shell.select_workspace(ws, &self.model);
        self.set_focus(Focus::List);
    }

    /// A click on the list panel's title row or its view switcher at `at`: the chip opens the
    /// workspace switcher, a view shows in the list panel, which gets the keyboard. `false`
    /// when nothing is there.
    pub(super) fn nav_click(&mut self, at: Position) -> bool {
        if let Some(chip) = self.chip_bar().filter(|b| b.area.y == at.y) {
            let (from, to) = chip.extent();
            if at.x >= from && at.x < to {
                self.open_switcher();
            }
            return true;
        }
        let Some(bar) = self.view_switcher().filter(|b| b.area.y == at.y) else { return false };
        if let Some(&v) = bar.hit(at.x).and_then(|i| View::ALL.get(i)) {
            self.shell.select(v, &self.model);
            self.set_focus(Focus::List);
        }
        true
    }

    /// The mouse over the switcher: a click on a workspace switches to it, outside closes it;
    /// the wheel moves the cursor. `true` when the screen changed.
    pub(super) fn switcher_mouse(&mut self, m: MouseEvent) -> bool {
        let Some(b) = self.switcher_box() else { return false };
        let at = Position { x: m.column, y: m.row };
        match m.kind {
            MouseEventKind::ScrollDown => self.switcher_key(ShellAction::SwitcherNext),
            MouseEventKind::ScrollUp => self.switcher_key(ShellAction::SwitcherPrev),
            MouseEventKind::Down(MouseButton::Left) if screen::inner(b).contains(at) => {
                let first = screen::switcher_first(self.switcher.unwrap_or(0), usize::from(screen::inner(b).height));
                let ws = first + usize::from(at.y - b.y - 1);
                if ws < self.model.workspaces().len() {
                    self.switch_to(ws);
                }
            }
            MouseEventKind::Down(_) if !b.contains(at) => self.switcher = None,
            _ => return false,
        }
        true
    }
}
