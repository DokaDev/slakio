//! The tabs as the app routes them: the tab actions ([`TabAction`]), where the keyboard goes
//! after each, what a tab is called and shows on the bar, and the mouse on the tab bar.
//!
//! | what | keys | mouse | the keyboard goes to |
//! |---|---|---|---|
//! | open in a new tab (focused instead when open) | `t` | | the new tab's pane |
//! | show another tab | `g t` `g T`, `Ctrl+PgDn/PgUp`, `Space 1..9`, `Alt+1..9` | click, `‹` `›` | its active pane |
//! | close a tab | `Space t c`, `Ctrl+W` on its last pane | `×`, middle click | the tab shown next (none left: the list) |
//! | reopen the tab closed last | `Space t u` (`Ctrl+O` with none open) | | its active pane |
//! | rename | `Space t r`, `:rename <name>` | double click | (the command line) |
//! | move | `Space t h` `Space t l` | drag | stays |
//!
//! A tab is called by the name the user gave it, else by what its active pane shows:
//! `#channel`, `@person`, `⤷ ` and the first line of a thread. Tabs other than the one shown
//! carry what their conversations hold unread, as the list counts it ([`Unread`]); nothing else
//! about them changes on its own, least of all their order.

use super::model::breadcrumb;
use super::pane::Pane;
use super::shell::Region;
use super::{App, Focus};
use crate::action::{Action, AppAction, TabAction};
use crate::keymap::Ctx;
use crate::keymap::hints::key_label;
use crate::tabbar::{self, Bar, Hit, Label as TabLabel};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use slakio_core::i18n::{Label, Msg};
use slakio_core::layout::tabs::Tab;
use slakio_core::model::Target;
use std::time::Instant;

/// What a tab's conversations hold unread, as the list panel shows it: their mentions (the
/// list's pill), the unread messages of DMs without mentions (a DM's pill), and whether any
/// conversation not muted is unread (a channel is bold, with no count).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unread {
    /// Mentions in channels (`@n`).
    pub mentions: u32,
    /// Unread messages of DMs (`●n`).
    pub dms: u32,
    pub any: bool,
    /// A DM of those mentions the user: its `●n` is red.
    pub dm_mention: bool,
}

impl Unread {
    /// The tab's mark: `@3` for mentions, `●2` for a DM's unread messages, `●` for unread
    /// channels; none when nothing is unread.
    pub fn badge(self) -> Option<String> {
        match self {
            Unread { mentions: m, .. } if m > 0 => Some(format!("@{m}")),
            Unread { dms: d, .. } if d > 0 => Some(format!("●{d}")),
            Unread { any: true, .. } => Some("●".to_string()),
            _ => None,
        }
    }

    /// How strong the mark is: 2 a mention, 1 unread, 0 nothing.
    pub fn level(self) -> u8 {
        if self.red() { 2 } else { u8::from(self.any) }
    }

    /// Something mentions the user: the mark is red.
    pub fn red(self) -> bool {
        self.mentions > 0 || self.dm_mention
    }
}

impl App {
    pub(super) fn tab(&mut self, a: TabAction, now: Instant) {
        if self.backend.is_none() {
            return;
        }
        let none = self.work.tabs().is_empty();
        match a {
            TabAction::Open => self.open_in_tab(now),
            _ if none && a != TabAction::Reopen => self.info(Msg::Label(Label::StatusNoTab), now),
            TabAction::Next | TabAction::Prev => {
                let by = if a == TabAction::Next { 1 } else { -1 };
                let id = self.work.step_tab(by);
                self.go_to(id);
            }
            TabAction::Go(n) => match self.work.select_tab(usize::from(n.max(1)) - 1) {
                Some(id) => self.go_to(Some(id)),
                None => self.info(Msg::StatusNoTabNumber { number: n.to_string() }, now),
            },
            TabAction::Close => {
                if let Some(i) = self.work.tabs().current() {
                    self.close_tab(i);
                }
            }
            TabAction::Reopen => match self.work.reopen() {
                Some(id) => self.go_to(Some(id)),
                None => self.info(Msg::Label(Label::StatusNoClosedTab), now),
            },
            TabAction::Rename => self.ask_rename(),
            TabAction::MoveLeft | TabAction::MoveRight => {
                let by = if a == TabAction::MoveRight { 1 } else { -1 };
                if !self.work.shift_tab(by) {
                    self.info(Msg::Label(Label::StatusTabEdge), now);
                }
            }
        }
    }

    /// `t`: the conversation under the list's cursor, or the selected message's thread, in a new
    /// tab (focused instead when open already), with the keyboard.
    fn open_in_tab(&mut self, now: Instant) {
        let target = match self.focus() {
            Focus::List => self.shell.target_at_cursor(&self.model),
            Focus::Pane(_) => {
                let thread = self.work.selected_thread();
                if thread.is_none() {
                    let keys = key_label(&self.keymap, Action::Tab(TabAction::Open), Ctx::PaneNormal)
                        .unwrap_or_else(|| "t".to_string());
                    self.info(Msg::StatusSelectMessage { keys }, now);
                }
                thread
            }
            Focus::ViewSwitcher | Focus::Work => None,
        };
        if let Some(t) = target {
            let id = self.work.open_tab(t);
            self.go_to(Some(id));
        }
    }

    /// The keyboard goes to pane `id` (a tab just shown).
    fn go_to(&mut self, id: Option<slakio_core::layout::PaneId>) {
        if let Some(id) = id {
            self.set_focus(Focus::on(id));
        }
    }

    /// Close tab `i` (its panes with it). The keyboard stays where it was: in the work area it
    /// goes to the tab shown next; with no tab left, to the list (on the conversation closed).
    pub(super) fn close_tab(&mut self, i: usize) {
        let in_work = self.region() == Region::Work;
        let closed = self.work.close_tab(i);
        match (self.work.active(), in_work) {
            (Some(id), true) => self.set_focus(Focus::on(id)),
            (None, true) => self.focus_list(closed),
            _ => {}
        }
    }

    /// Close the work area's active pane (`Ctrl+W`, `:q`): its tab with its last pane, never the
    /// app. From a pane the keyboard goes to the pane shown next, else to the list (on the
    /// conversation closed); from the list or the view switcher it stays there. With nothing open, says
    /// how to quit.
    pub(super) fn close_pane(&mut self, now: Instant) {
        if self.work.active().is_none() {
            let keys = key_label(&self.keymap, Action::App(AppAction::Quit), Ctx::Root).unwrap_or_default();
            return self.info(Msg::StatusNothingToClose { keys }, now);
        }
        let in_pane = self.focus().is_pane();
        let (closed, next) = self.work.close();
        if !in_pane {
            return;
        }
        if let Some(id) = next {
            self.set_focus(Focus::on(id));
        }
        if let Some(closed) = closed {
            self.focus_list(Some(closed));
        }
    }

    /// The command line, `rename ` and the tab's name typed in, to edit and run.
    fn ask_rename(&mut self) {
        let name = self.work.tabs().current_tab().and_then(|t| t.name.clone()).unwrap_or_default();
        self.help = None;
        self.cmdline.open();
        self.cmdline.set(&format!("rename {name}"));
    }

    /// What pane `p` is called on a tab: `#backend`, `@Minsu Kim`, `⤷ ` and its thread's first
    /// line (the conversation's name until the thread is loaded).
    fn pane_title(&self, p: &Pane) -> String {
        let name = self.model.target(&p.target).map(breadcrumb).unwrap_or_default();
        let Target::Thread { workspace, conversation, thread } = &p.target else { return name };
        // The thread's message: in the thread once its first page is loaded, else in its
        // conversation, when that is loaded.
        let parent = Target::Conversation { workspace: workspace.clone(), conversation: conversation.clone() };
        let first = [Some(self.work.timeline(p)), self.work.timelines.get(&parent)]
            .into_iter()
            .flatten()
            .find_map(|tl| tl.position(*thread).map(|i| &tl.items[i]))
            .and_then(|m| m.text.as_str().lines().find(|l| !l.trim().is_empty()));
        format!("⤷ {}", first.map_or(name, |l| l.trim().to_string()))
    }

    /// The title of `tab`: its name, else what its active pane is called.
    pub fn tab_title(&self, tab: &Tab) -> String {
        if let Some(name) = &tab.name {
            return name.clone();
        }
        self.work.pane(tab.active).map(|p| self.pane_title(p)).unwrap_or_default()
    }

    /// What the conversations `tab` shows hold unread, counted as the list's pills count it.
    pub fn tab_unread(&self, tab: &Tab) -> Unread {
        let mut seen: Vec<&Target> = Vec::new();
        let mut u = Unread::default();
        for id in tab.root.leaves() {
            let Some(p) = self.work.pane(id) else { continue };
            let Some(c) = self.model.target(&p.target).filter(|_| !p.is_thread()) else { continue };
            if seen.contains(&&p.target) {
                continue;
            }
            seen.push(&p.target);
            if c.is_dm() {
                // A DM counts its unread messages; a mention in it only makes the mark red.
                if c.unread > 0 && !c.muted {
                    u.any = true;
                    u.dms += c.unread;
                }
                u.dm_mention |= c.mentions > 0;
            } else {
                u.mentions += c.mentions;
                u.any |= c.unread > 0 && !c.muted;
            }
        }
        u
    }

    /// The tab bar as laid out now, while it shows (two tabs or more).
    pub fn tab_bar(&self) -> Option<Bar> {
        let area = self.areas().tabs?;
        let tabs = self.work.tabs();
        let current = tabs.current()?;
        let labels: Vec<TabLabel> = tabs
            .all()
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let badge = self.tab_unread(t).badge().filter(|_| i != current);
                TabLabel { number: (i + 1).to_string(), title: self.tab_title(t), badge }
            })
            .collect();
        Some(tabbar::layout(area, &labels, current))
    }

    /// The mouse on the tab bar at column `x`: a click shows the tab (twice: rename it), on `×`
    /// closes it, on `‹` / `›` shows the nearest tab left out; a middle click closes the tab; a
    /// drag moves the tab pressed along. `true` when the screen changed.
    pub(super) fn tab_bar_mouse(&mut self, m: MouseEvent, double: bool) -> bool {
        let Some(bar) = self.tab_bar() else { return false };
        let hit = bar.hit(m.column);
        match (m.kind, hit) {
            (MouseEventKind::Down(MouseButton::Left), Some(Hit::Close(i))) => self.close_tab(i),
            (MouseEventKind::Down(MouseButton::Left), Some(Hit::Tab(i) | Hit::More(i))) => {
                self.tab_drag = matches!(hit, Some(Hit::Tab(_))).then_some(i);
                let id = self.work.select_tab(i);
                self.go_to(id);
                if double && matches!(hit, Some(Hit::Tab(_))) {
                    self.ask_rename();
                }
            }
            (MouseEventKind::Down(MouseButton::Middle), Some(Hit::Tab(i) | Hit::Close(i))) => self.close_tab(i),
            (MouseEventKind::Drag(MouseButton::Left), _) => {
                let (Some(from), Some(to)) = (self.tab_drag, bar.tab_at(m.column)) else { return false };
                if !self.work.move_tab(from, to) {
                    return false;
                }
                self.tab_drag = Some(to);
            }
            _ => return false,
        }
        true
    }
}
