//! The top bar as the app routes it: what its chip and views say, the workspace switcher it
//! opens, and the mouse on both. The geometry is [`crate::navbar`]'s.
//!
//! The keyboard reaches the bar with `Ctrl+R` / `Space r` (`:nav`), or `Tab` round the panels;
//! `h`/`l` and the arrows move along it, `Enter` shows the view under the cursor in the list
//! panel (on the workspace: opens the switcher), `Esc` goes back. `Space W` opens the switcher
//! from anywhere outside text; in it `j`/`k` move, `Enter` switches, `Esc` closes.
//!
//! Counts (ui-ux-spec §H): Home `@n` the mentions in its channels, else `●` when a channel is
//! unread; DMs `●n` the unread messages of its DMs; Activity `@n` the mentions of every
//! workspace. Only mentions are `@` and red. Another workspace's mark follows `▾` with its letter.

use super::shell::{NavItem, View, nav_items};
use super::tabs::Unread;
use super::{App, Focus};
use crate::action::ShellAction;
use crate::navbar::{self, Bar, Chip, Item};
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

    /// The top bar as laid out now (`None` without a backend).
    pub fn nav_bar(&self) -> Option<Bar> {
        self.backend?;
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
        Some(navbar::layout(self.areas().nav, &chip, &views))
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
        let x = self.nav_bar().and_then(|b| b.span(0)).map_or(0, |(from, _)| from);
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

    /// Enter on the top bar: the workspace opens the switcher; a view the shell shows.
    pub(super) fn nav_select(&mut self) -> bool {
        let on_chip = nav_items().get(self.shell.nav_cursor) == Some(&NavItem::Workspace);
        if on_chip {
            self.open_switcher();
        }
        on_chip
    }

    /// A click on the top bar at column `x`: the chip opens the switcher, a view shows in the
    /// list panel, which gets the keyboard.
    pub(super) fn nav_click(&mut self, x: u16) {
        let Some(i) = self.nav_bar().and_then(|b| b.hit(x)) else { return };
        let items = nav_items();
        self.shell.nav_cursor = i;
        match items.get(i) {
            Some(NavItem::Workspace) => self.open_switcher(),
            Some(item) => {
                self.shell.select(*item, &items, &self.model);
                self.set_focus(Focus::List);
            }
            None => {}
        }
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
