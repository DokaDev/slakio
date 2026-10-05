//! The keyboard help: every context's keys, made from the key map and the action registry, so
//! it always shows what the keys do now. The context it was opened from and its parents are
//! open first; the others are folded with their number of keys. `/` filters the rows; `Enter`
//! runs the action of a row (or opens a section).

use crate::action::{self, Action};
use crate::keymap::{Ctx, Keymap, keys};
use slakio_core::i18n::{I18n, Label};
use std::cell::Cell;
use std::collections::HashSet;

/// One row of the help.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A context's header, open or folded, with its number of keys.
    Section { ctx: Ctx, open: bool, count: usize },
    /// An action of a context and every key that runs it there (` / ` between them).
    Entry { ctx: Ctx, action: Action, label: Label, keys: String },
}

#[derive(Clone, Debug)]
pub struct Help {
    /// Where the keyboard was when the help opened.
    pub origin: Ctx,
    pub open: HashSet<Ctx>,
    /// The row under the cursor.
    pub cursor: usize,
    /// The first row on screen. Kept by drawing, like the rows the box shows.
    pub top: Cell<usize>,
    pub height: Cell<usize>,
    /// What filters the rows.
    pub filter: String,
    /// The filter is being typed.
    pub typing: bool,
}

impl Help {
    pub fn new(origin: Ctx) -> Self {
        let mut open: HashSet<Ctx> = origin.chain().into_iter().collect();
        open.insert(Ctx::Global);
        Self { origin, open, cursor: 0, top: Cell::new(0), height: Cell::new(10), filter: String::new(), typing: false }
    }

    /// The contexts in the order the help lists them: where it was opened from and its parents,
    /// the global keys, then the others.
    fn order(&self) -> Vec<Ctx> {
        let mut out = self.origin.chain();
        out.push(Ctx::Global);
        let rest: Vec<Ctx> = Ctx::ALL.iter().copied().filter(|c| !out.contains(c)).collect();
        out.extend(rest);
        out
    }

    /// The rows now. With a filter, only the keys whose label or keys contain it (any case),
    /// under their open sections.
    pub fn rows(&self, km: &Keymap, i18n: &I18n) -> Vec<Row> {
        let needle = self.filter.to_lowercase();
        let mut out = Vec::new();
        for ctx in self.order() {
            let mut entries: Vec<Row> = Vec::new();
            for (k, a) in km.bindings(ctx) {
                let shown = keys::label(k);
                match entries.iter_mut().find(|e| matches!(e, Row::Entry { action, .. } if *action == a)) {
                    Some(Row::Entry { keys, .. }) => {
                        keys.push_str(" / ");
                        keys.push_str(&shown);
                    }
                    _ => entries.push(Row::Entry { ctx, action: a, label: action::spec(a).label, keys: shown }),
                }
            }
            if !needle.is_empty() {
                entries.retain(|e| match e {
                    Row::Entry { label, keys, .. } => {
                        i18n.label(*label).to_lowercase().contains(&needle) || keys.to_lowercase().contains(&needle)
                    }
                    Row::Section { .. } => false,
                });
            }
            if entries.is_empty() {
                continue;
            }
            let open = !needle.is_empty() || self.open.contains(&ctx);
            out.push(Row::Section { ctx, open, count: entries.len() });
            if open {
                out.extend(entries);
            }
        }
        out
    }

    /// Move the cursor by `by` rows within `n` rows.
    pub fn step(&mut self, by: isize, n: usize) {
        self.cursor = self.cursor.saturating_add_signed(by).min(n.saturating_sub(1));
    }

    /// Open (`true`) or fold the section of the row under the cursor; the cursor goes to its
    /// header.
    pub fn set_open(&mut self, rows: &[Row], open: bool) {
        let at = self.cursor.min(rows.len().saturating_sub(1));
        let Some(header) =
            rows[..=at.min(rows.len().saturating_sub(1))].iter().rposition(|r| matches!(r, Row::Section { .. }))
        else {
            return;
        };
        if let Row::Section { ctx, .. } = rows[header] {
            if open {
                self.open.insert(ctx);
            } else {
                self.open.remove(&ctx);
                self.cursor = header;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{AppAction, HelpAction, ShellAction};
    use slakio_core::i18n::Lang;

    #[test]
    fn the_help_opens_where_the_keyboard_is_and_folds_the_rest() {
        let km = Keymap::default();
        let i18n = I18n::new(Lang::En);
        let h = Help::new(Ctx::List);
        let rows = h.rows(&km, &i18n);
        assert_eq!(rows[0], Row::Section { ctx: Ctx::List, open: true, count: rows_of(&rows, Ctx::List) });
        let next = rows
            .iter()
            .find(|r| matches!(r, Row::Entry { action: Action::Shell(ShellAction::ListNext), .. }))
            .expect("the list's keys");
        assert!(matches!(next, Row::Entry { keys, .. } if keys == "j / Down"), "{next:?}");
        assert!(rows.iter().any(|r| matches!(r, Row::Section { ctx: Ctx::Rail, open: false, count } if *count > 0)));
        let quit = rows.iter().any(|r| matches!(r, Row::Entry { action: Action::App(AppAction::Quit), .. }));
        assert!(quit, "the global keys are open too");
    }

    fn rows_of(rows: &[Row], ctx: Ctx) -> usize {
        rows.iter().filter(|r| matches!(r, Row::Entry { ctx: c, .. } if *c == ctx)).count()
    }

    #[test]
    fn a_filter_keeps_the_matching_keys_of_every_context() {
        let km = Keymap::default();
        let i18n = I18n::new(Lang::En);
        let mut h = Help::new(Ctx::List);
        h.filter = "HELP".into();
        let rows = h.rows(&km, &i18n);
        assert!(rows.iter().all(|r| match r {
            Row::Entry { label, keys, .. } =>
                i18n.label(*label).to_lowercase().contains("help") || keys.contains("help"),
            Row::Section { open, .. } => *open,
        }));
        assert!(rows.iter().any(|r| matches!(r, Row::Entry { action: Action::Help(HelpAction::Open), .. })));
        h.filter = "no such key".into();
        assert!(h.rows(&km, &i18n).is_empty());
    }

    #[test]
    fn sections_open_and_fold() {
        let km = Keymap::default();
        let i18n = I18n::new(Lang::En);
        let mut h = Help::new(Ctx::List);
        let rows = h.rows(&km, &i18n);
        h.cursor = 2;
        h.set_open(&rows, false);
        assert_eq!(h.cursor, 0, "the cursor goes to the header");
        let folded = h.rows(&km, &i18n);
        assert!(matches!(folded[0], Row::Section { open: false, .. }));
        h.set_open(&folded, true);
        assert_eq!(h.rows(&km, &i18n), rows);
    }
}
