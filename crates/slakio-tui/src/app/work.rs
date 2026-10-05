//! The work area in the default (GUI Slack) mode: the conversation open in the main pane, the
//! auto thread panel beside it, which of the two has the keyboard, whether its composer is
//! being written in, and the back/forward history of the main pane.
//!
//! ```text
//! ╭ #backend ───────────────╮╭ ⤷ Thread ─────────╮
//! │ Kim  Starting deploy    ││ Kim  Starting …   │
//! │      ⤷ 4 replies        ││ Park Confirmed    │
//! │╭ Message #backend ─────╮││╭ Reply ─────────╮ │
//! ```
//!
//! `Enter` on a message opens its thread in the panel, replacing what it showed. Opening a
//! conversation that is already open focuses it instead of loading it again. Pages of messages
//! are asked for through [`Work::take_requests`]; an answer for an older request or another
//! target is dropped.

use super::model::Model;
use super::pane::{PAGE, Pane, Shown};
use slakio_core::backend::{Command, Generation, Page};
use slakio_core::model::{Message, Target};
use slakio_core::sanitize::{Safe, sanitize_block, sanitize_line};

/// Targets the back history keeps.
const HISTORY: usize = 50;

/// One of the two panes of the work area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Main,
    Thread,
}

#[derive(Clone, Debug)]
pub struct Work {
    pub main: Option<Pane>,
    /// The auto thread panel.
    pub thread: Option<Pane>,
    /// Which pane has the keyboard when the work area has the focus.
    pub side: Side,
    /// The focused pane's composer is being written in (Insert mode).
    pub insert: bool,
    back: Vec<Target>,
    forward: Vec<Target>,
    generation: Generation,
    requests: Vec<(Generation, Command)>,
}

impl Default for Work {
    fn default() -> Self {
        Self {
            main: None,
            thread: None,
            side: Side::Main,
            insert: false,
            back: Vec::new(),
            forward: Vec::new(),
            generation: Generation::default(),
            requests: Vec::new(),
        }
    }
}

/// `m` as drawn in a pane of `model`.
fn shown(model: &Model, target: &Target, m: &Message) -> Shown {
    let author = model.user(&m.user).map_or_else(|| sanitize_line(m.user.as_str()), |u| u.display_name.line());
    let own = model.me(target.workspace()) == Some(&m.user);
    Shown::new(m, author, own)
}

impl Work {
    /// The pane with the keyboard.
    pub fn focused(&self) -> Option<&Pane> {
        match self.side {
            Side::Thread => self.thread.as_ref(),
            Side::Main => self.main.as_ref(),
        }
    }

    pub fn focused_mut(&mut self) -> Option<&mut Pane> {
        match self.side {
            Side::Thread => self.thread.as_mut(),
            Side::Main => self.main.as_mut(),
        }
    }

    /// The page requests queued since the last call.
    pub fn take_requests(&mut self) -> Vec<(Generation, Command)> {
        std::mem::take(&mut self.requests)
    }

    /// Ask for the pages the panes want.
    fn fill(&mut self) {
        for pane in [self.main.as_mut(), self.thread.as_mut()].into_iter().flatten() {
            if let Some(before) = pane.wants() {
                self.generation = self.generation.next();
                pane.pending = Some(self.generation);
                let command = Command::History { target: pane.target.clone(), before, limit: PAGE };
                self.requests.push((self.generation, command));
            }
        }
    }

    /// Open the conversation `target` in the main pane (focusing it if it is open already).
    pub fn open(&mut self, target: Target) {
        if let Some(current) = self.main.as_ref().map(|p| p.target.clone()) {
            if current == target {
                self.side = Side::Main;
                return;
            }
            self.back.push(current);
            if self.back.len() > HISTORY {
                self.back.remove(0);
            }
        }
        self.forward.clear();
        self.show(target);
    }

    fn show(&mut self, target: Target) {
        self.main = Some(Pane::new(target));
        self.thread = None;
        self.side = Side::Main;
        self.insert = false;
        self.fill();
    }

    /// Back to the conversation before (`Ctrl+O`). `false` when there is none.
    pub fn back(&mut self) -> bool {
        let Some(to) = self.back.pop() else { return false };
        if let Some(p) = self.main.as_ref() {
            self.forward.push(p.target.clone());
        }
        self.show(to);
        true
    }

    /// Forward again (`Ctrl+I`). `false` when there is none.
    pub fn forward(&mut self) -> bool {
        let Some(to) = self.forward.pop() else { return false };
        if let Some(p) = self.main.as_ref() {
            self.back.push(p.target.clone());
        }
        self.show(to);
        true
    }

    /// Open the thread of the selected message of the main pane in the thread panel (replacing
    /// what it showed; focusing it when it shows that thread already).
    pub fn open_thread(&mut self) {
        if self.side != Side::Main {
            return;
        }
        let Some(main) = self.main.as_ref() else { return };
        let Some(m) = main.selected.and_then(|i| main.items.get(i)) else { return };
        let target = main.thread_target(m.ts);
        self.side = Side::Thread;
        if self.thread.as_ref().is_some_and(|t| t.target == target) {
            return;
        }
        self.thread = Some(Pane::new(target));
        self.fill();
    }

    /// Close the focused pane (`Ctrl+W`): the thread panel, else the conversation.
    pub fn close(&mut self) {
        self.insert = false;
        match self.side {
            Side::Thread => {
                self.thread = None;
                self.side = Side::Main;
            }
            Side::Main => {
                self.main = None;
                self.thread = None;
            }
        }
    }

    /// Move the keyboard to the pane on the left (`-1`) or right (`1`). `false` when there is
    /// none that way (the shell moves the focus out of the work area then).
    pub fn focus_side(&mut self, step: i8) -> bool {
        match (self.side, step) {
            (Side::Main, 1) if self.thread.is_some() => self.side = Side::Thread,
            (Side::Thread, -1) => self.side = Side::Main,
            _ => return false,
        }
        self.insert = false;
        true
    }

    /// A page of messages arrived. `true` when a pane took it; an answer to an older request
    /// or for a target no pane shows is dropped.
    pub fn on_page(&mut self, generation: Generation, page: &Page, model: &Model) -> bool {
        let pane = [self.main.as_mut(), self.thread.as_mut()]
            .into_iter()
            .flatten()
            .find(|p| p.target == page.target && p.pending == Some(generation));
        let Some(pane) = pane else { return false };
        let shown = page.messages.iter().map(|m| shown(model, &page.target, m)).collect();
        pane.add_page(page, shown);
        self.fill();
        true
    }

    /// Run `f` on the focused pane, then ask for the pages it now wants.
    pub fn with_pane(&mut self, f: impl FnOnce(&mut Pane)) {
        if let Some(p) = self.focused_mut() {
            f(p);
        }
        self.fill();
    }

    /// Send what the focused pane's composer holds. The demo only echoes it locally: it is
    /// shown as the user's message and goes nowhere. `false` when there was nothing to send.
    pub fn send(&mut self, model: &Model) -> bool {
        let Some(pane) = self.focused_mut() else { return false };
        if pane.composer.text().trim().is_empty() {
            return false;
        }
        let text = pane.composer.take();
        let workspace = pane.target.workspace().clone();
        let Some(me) = model.me(&workspace).cloned() else { return false };
        let author = model.user(&me).map_or_else(Safe::default, |u| u.display_name.line());
        pane.echo(me, author, sanitize_block(&text));
        true
    }

    /// Drop what no longer exists after a new boot answer.
    pub fn clamp(&mut self, model: &Model) {
        if self.main.as_ref().is_some_and(|p| model.target(&p.target).is_none()) {
            self.main = None;
            self.thread = None;
            self.side = Side::Main;
            self.insert = false;
        }
        self.back.retain(|t| model.target(t).is_some());
        self.forward.retain(|t| model.target(t).is_some());
    }
}

#[cfg(test)]
mod tests;
