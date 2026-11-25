//! The list panel's rows of a view.

use super::super::shell::View;
use super::super::todos::{Group, TodoView};
use super::{Model, Row};
use slakio_core::model::{SectionId, SectionKind, WorkspaceId};
use std::collections::HashSet;

impl Model {
    /// The rows of `view` in workspace `ws`, sections in `collapsed` and groups in `groups`
    /// folded, To-dos as `opts` filters and orders them.
    pub fn rows(
        &self,
        ws: &WorkspaceId,
        view: View,
        collapsed: &HashSet<SectionId>,
        groups: &HashSet<Group>,
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
                    // As Slack's desktop app: a closed DM is listed only in a section of the
                    // user's own and Starred.
                    let keeps_closed = matches!(s.kind, SectionKind::Favorites | SectionKind::Custom);
                    let shown = convs.clone().filter(|(_, c)| c.section == s.id && (keeps_closed || !c.closed));
                    // Starred, External connections and Apps show only when they list a
                    // conversation; the others always do.
                    let hides = matches!(s.kind, SectionKind::Favorites | SectionKind::External | SectionKind::Apps);
                    if hides && shown.clone().next().is_none() {
                        continue;
                    }
                    if !rows.is_empty() {
                        rows.push(Row::Spacer);
                    }
                    rows.push(Row::Section(si));
                    if !collapsed.contains(&s.id) {
                        rows.extend(shown.map(|(i, _)| Row::Conversation(i)));
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
            View::Dms => convs.filter(|(_, c)| c.is_dm()).map(|(i, _)| Row::Conversation(i)).collect(),
            View::Activity => self.notice_rows(),
            // Not built yet: the list panel says so.
            View::Files => vec![],
        }
    }
}
