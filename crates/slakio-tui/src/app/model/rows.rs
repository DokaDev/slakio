//! The list panel's rows of a view: a section's conversations in the order it takes
//! ([`Sorting`]), the DMs view's in the order `dm_sort` gives.

use super::super::shell::View;
use super::super::todos::{Group, TodoView};
use super::{Model, Row};
use slakio_core::model::{Conversation, Section, SectionId, SectionKind, Sort, WorkspaceId};
use std::collections::{HashMap, HashSet};

/// Section `s` lists `c`: it is in `s`, and, as in Slack's desktop app, a closed DM is listed only
/// in a section of the user's own and Starred.
pub(super) fn lists(s: &Section, c: &Conversation) -> bool {
    let keeps_closed = matches!(s.kind, SectionKind::Favorites | SectionKind::Custom);
    c.workspace == s.workspace && c.section == s.id && (keeps_closed || !c.closed)
}

/// How the list orders the conversations of a section that is ordered ([`Section::sort`]; one
/// that is not keeps the order listed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sorting {
    /// Every section but Direct messages and Apps (`channel_sort`).
    pub channels: Sort,
    /// Direct messages and Apps, and the DMs view (`dm_sort`).
    pub dms: Sort,
    /// The user's own choice for a section (`:sort-section`), over both.
    pub sections: HashMap<SectionId, Sort>,
}

impl Default for Sorting {
    fn default() -> Self {
        Self { channels: Sort::Alphabetical, dms: Sort::Recent, sections: HashMap::new() }
    }
}

impl Sorting {
    /// The order section `s` takes; `None`: the order listed.
    pub fn of(&self, s: &Section) -> Option<Sort> {
        s.sort?;
        let setting = match s.kind {
            SectionKind::DirectMessages | SectionKind::Apps => self.dms,
            SectionKind::Favorites | SectionKind::Custom | SectionKind::External | SectionKind::Channels => {
                self.channels
            }
        };
        Some(self.sections.get(&s.id).copied().unwrap_or(setting))
    }
}

/// `convs` in `order` (stable: those alike keep the order listed): by name, case ignored; or the
/// newest first, those whose newest message is not known last.
fn ordered(mut convs: Vec<(usize, &Conversation)>, order: Option<Sort>) -> Vec<(usize, &Conversation)> {
    match order {
        None => {}
        Some(Sort::Alphabetical) => convs.sort_by_cached_key(|(_, c)| c.name.unsanitized().to_lowercase()),
        Some(Sort::Recent) => convs.sort_by_key(|(_, c)| std::cmp::Reverse(c.latest)),
    }
    convs
}

impl Model {
    /// The rows of `view` in workspace `ws`, sections in `collapsed` and groups in `groups`
    /// folded, conversations in the order `sorting` gives their section, To-dos as `opts`
    /// filters and orders them.
    pub fn rows(
        &self,
        ws: &WorkspaceId,
        view: View,
        (collapsed, groups): (&HashSet<SectionId>, &HashSet<Group>),
        sorting: &Sorting,
        opts: &TodoView,
    ) -> Vec<Row> {
        if self.workspace(ws).is_none() {
            return vec![];
        }
        let convs = self.snapshot.conversations.iter().enumerate().filter(|(_, c)| &c.workspace == ws);
        match view {
            View::Home => {
                let mut rows = vec![];
                for (si, s) in self.snapshot.sections.iter().enumerate().filter(|(_, s)| &s.workspace == ws) {
                    let shown = convs.clone().filter(|(_, c)| lists(s, c));
                    let shown = ordered(shown.collect(), sorting.of(s));
                    // Starred, External connections and Apps show only when they list a
                    // conversation; the others always do.
                    let hides = matches!(s.kind, SectionKind::Favorites | SectionKind::External | SectionKind::Apps);
                    if hides && shown.is_empty() {
                        continue;
                    }
                    if !rows.is_empty() {
                        rows.push(Row::Spacer);
                    }
                    rows.push(Row::Section(si));
                    if !collapsed.contains(&s.id) {
                        rows.extend(shown.into_iter().map(|(i, _)| Row::Conversation(i)));
                    }
                }
                // Saved messages, after the sections (Slack's Home "Saved").
                let saved = self.group_items(ws, Group::Saved, opts);
                if !saved.is_empty() {
                    rows.push(Row::Spacer);
                    rows.push(Row::Group(Group::Saved));
                    if !groups.contains(&Group::Saved) {
                        rows.extend(saved.into_iter().map(Row::Item));
                    }
                }
                rows
            }
            View::Todos => self.todo_rows(ws, groups, opts),
            // In the order `dm_sort` gives, when Direct messages are ordered at all; a choice of
            // `:sort-section` for Direct messages orders that section alone, not this view.
            View::Dms => {
                let dms =
                    self.snapshot.sections.iter().find(|s| &s.workspace == ws && s.kind == SectionKind::DirectMessages);
                let order = dms.and_then(|s| sorting.of(s)).map(|_| sorting.dms);
                ordered(convs.filter(|(_, c)| c.is_dm()).collect(), order)
                    .into_iter()
                    .map(|(i, _)| Row::Conversation(i))
                    .collect()
            }
            View::Activity => self.notice_rows(),
            // Not built yet: the list panel says so.
            View::Files => vec![],
        }
    }
}
