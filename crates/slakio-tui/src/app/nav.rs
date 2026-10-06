//! Navigation in the list panel as the app routes it: what the workspace chip (the panel's
//! title) and the views (its first rows, one per view, or one folded row) say, the workspace
//! switcher the chip opens, and the mouse on both. The geometry is [`crate::navbar`]'s.
//!
//! The views' rows are the top of the list panel, with a keyboard cursor of their own (the
//! focus region [`Focus::ViewSwitcher`]): `Ctrl+R` / `Space r` (`:nav`) or `Tab` round the
//! panels put it on the view shown; `j`/`k` move down and up the rows and on into the list (`k`
//! on the list's first row comes back up), `Enter` shows the view under the cursor (on the
//! folded row: unfolds it), `Esc` goes back to the list. In the list, `[` / `]` show the view
//! before or after. `Space v` (`:navrows`) folds the rows to the view shown or unfolds them
//! (`nav_rows`, saved).
//! `Space W` opens the workspace switcher from anywhere outside text; in it `j`/`k` move,
//! `Enter` switches, `Esc` closes.
//!
//! Counts (ui-ux-spec §H.1): Home `@n` the mentions in its channels, else `●` when a channel is
//! unread; DMs `●n` the unread messages of its DMs; Activity `@n` the mentions of every
//! workspace. Only mentions are `@` and red. Another workspace's mark follows `▾` with its letter.

use super::shell::View;
use super::tabs::Unread;
use super::{App, Effect, Focus};
use crate::action::ShellAction;
use crate::navbar::{self, Bar, Chip, Item, Part};
use crate::screen::{self, ListParts};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use slakio_core::i18n::Msg;
use std::time::Instant;

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
        let title = self.list_parts()?.title;
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
        Some(navbar::chip(title, &chip))
    }

    /// How many rows the views take at the top of the list panel: one each, or one folded.
    pub fn nav_rows(&self) -> u16 {
        if self.shell.nav_folded { 1 } else { View::ALL.len() as u16 }
    }

    /// The parts of the list panel as laid out now (`None` while it is not shown).
    pub fn list_parts(&self) -> Option<ListParts> {
        self.areas().list.map(|l| screen::list_parts(l, self.nav_rows()))
    }

    /// The views' rows as laid out now at the top of the list panel (empty without a backend or
    /// while the list panel is not shown).
    pub fn view_rows(&self) -> Vec<Bar> {
        let Some(parts) = self.list_parts().filter(|_| self.backend.is_some()) else { return Vec::new() };
        let icons = self.settings.icons;
        let views: Vec<Item> = View::ALL
            .iter()
            .map(|v| Item {
                glyph: icons.then(|| v.glyph()),
                label: self.i18n.label(v.label()).to_string(),
                badge: self.view_unread(self.shell.workspace, *v).badge(),
            })
            .collect();
        let shown = View::ALL.iter().position(|v| *v == self.shell.view).unwrap_or(0);
        navbar::views(parts.views, &views, shown, self.shell.nav_folded)
    }

    /// Fold the views to one row, the view shown (`nav_rows = "collapsed"`), or unfold them.
    pub fn fold_views(&mut self, folded: bool) {
        self.shell.nav_folded = folded;
        self.shell.nav_cursor = View::ALL.iter().position(|v| *v == self.shell.view).unwrap_or(0);
    }

    /// `Space v` (`:navrows`): fold the views to the view shown, or unfold them, and save it
    /// in the config file (`nav_rows`).
    pub(super) fn toggle_nav_rows(&mut self, now: Instant) {
        self.fold_views(!self.shell.nav_folded);
        let value = if self.shell.nav_folded { "collapsed" } else { "expanded" };
        self.effects.push(Effect::Save { key: "nav_rows", value: value.to_string() });
        self.info(Msg::NavRowsChanged { name: value.to_string() }, now);
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

    /// A click on the list panel's title row or a view's row at `at`: the chip opens the
    /// workspace switcher, a view shows in the list panel, which gets the keyboard; the folded
    /// row unfolds. `false` when nothing is there.
    pub(super) fn nav_click(&mut self, at: Position, now: Instant) -> bool {
        if let Some(chip) = self.chip_bar().filter(|b| b.area.y == at.y) {
            let (from, to) = chip.extent();
            if at.x >= from && at.x < to {
                self.open_switcher();
            }
            return true;
        }
        let Some(row) = self.view_rows().into_iter().find(|b| b.area.y == at.y) else { return false };
        if self.shell.nav_folded {
            self.toggle_nav_rows(now);
        } else if let Some(&v) = row.hit(at.x).and_then(|i| View::ALL.get(i)) {
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
