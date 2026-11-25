//! How the sidebar's sections order their conversations: the settings (`channel_sort`,
//! `dm_sort`) and the user's choice for one section (`:sort-section`, which switches the section
//! under the list's cursor between by name and the newest first), kept with the layout as each
//! workspace's sidebar ([`SavedSidebar`]) with the sections folded.

use super::App;
use super::model::{Row, Sorting};
use slakio_core::config::Config;
use slakio_core::i18n::{Label, Msg};
use slakio_core::layout::saved::SavedSidebar;
use slakio_core::model::Sort;
use slakio_core::model::{SectionId, WorkspaceId};
use std::collections::BTreeMap;
use std::time::Instant;

impl App {
    /// The orders the config file `cfg` sets (a value it cannot have is the default's).
    pub fn use_sorting(&mut self, cfg: &Config) {
        let defaults = Sorting::default();
        self.sort_sections(|s| {
            s.channels = Sort::named(&cfg.channel_sort).unwrap_or(defaults.channels);
            s.dms = Sort::named(&cfg.dm_sort).unwrap_or(defaults.dms);
        });
    }

    /// Change the orders by `change`; the list's cursor stays on its row.
    pub(super) fn sort_sections(&mut self, change: impl FnOnce(&mut Sorting)) {
        let anchor = self.shell.anchor(&self.model);
        change(&mut self.shell.sorting);
        let h = self.list_height();
        self.shell.clamp(&self.model, h);
        self.shell.restore(anchor, &self.model, h);
    }

    /// `:sort-section`: the section under the list's cursor (its header, or the section of the
    /// conversation there) by name when it is ordered the newest first, else the newest first.
    /// A section that keeps the order listed, or none under the cursor, says so.
    pub(super) fn sort_section(&mut self, now: Instant) {
        let rows = self.shell.rows(&self.model);
        let section = match rows.get(self.shell.list_cursor) {
            Some(Row::Section(i)) => Some(self.model.section(*i)),
            // The header of the conversation's section, among the rows.
            Some(Row::Conversation(i)) => {
                let at = &self.model.conversation(*i).section;
                rows.iter().find_map(|r| match r {
                    Row::Section(k) if self.model.section(*k).id == *at => Some(self.model.section(*k)),
                    _ => None,
                })
            }
            _ => None,
        };
        let Some((s, now_order)) = section.and_then(|s| Some((s.clone(), self.shell.sorting.of(s)?))) else {
            return self.info(Msg::Label(Label::SectionSortNone), now);
        };
        let next = if now_order == Sort::Recent { Sort::Alphabetical } else { Sort::Recent };
        self.sort_sections(|sorting| {
            sorting.sections.insert(s.id.clone(), next);
        });
        self.shell.sidebar_edits += 1;
        // Said in the UI's language, as the settings name it.
        let named = if next == Sort::Recent { Label::SettingsSortRecent } else { Label::SettingsSortAlphabetical };
        self.info(Msg::SectionSortChanged { name: self.i18n.label(named).to_string() }, now);
    }

    /// Each workspace's sidebar to keep: the sections folded and those the user ordered, by
    /// workspace (one the model lists the section of, else the one the layout file named it
    /// with), each list in id order; a workspace with neither is left out.
    pub(super) fn sidebar_to_save(&self) -> Vec<SavedSidebar> {
        let home = |id: &SectionId| -> Option<WorkspaceId> {
            let listed = self.model.sections().iter().find(|s| s.id == *id).map(|s| s.workspace.clone());
            listed.or_else(|| self.shell.sidebar_homes.get(id).cloned())
        };
        let mut by: BTreeMap<WorkspaceId, SavedSidebar> = BTreeMap::new();
        for id in &self.shell.collapsed {
            if let Some(ws) = home(id) {
                by.entry(ws.clone())
                    .or_insert_with(|| SavedSidebar { workspace: ws, collapsed: Vec::new(), sort: Vec::new() })
                    .collapsed
                    .push(id.clone());
            }
        }
        for (id, order) in &self.shell.sorting.sections {
            if let Some(ws) = home(id) {
                by.entry(ws.clone())
                    .or_insert_with(|| SavedSidebar { workspace: ws, collapsed: Vec::new(), sort: Vec::new() })
                    .sort
                    .push((id.clone(), *order));
            }
        }
        let mut out: Vec<SavedSidebar> = by.into_values().collect();
        for s in &mut out {
            s.collapsed.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
            s.sort.sort_unstable_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        }
        out
    }

    /// The sidebars the layout file kept, folded and ordered again (before the workspaces have
    /// answered: by the sections' ids).
    pub(super) fn restore_sidebar(&mut self, saved: &[SavedSidebar]) {
        for s in saved {
            for id in &s.collapsed {
                self.shell.collapsed.insert(id.clone());
                self.shell.sidebar_homes.insert(id.clone(), s.workspace.clone());
            }
            for (id, order) in &s.sort {
                self.shell.sorting.sections.insert(id.clone(), *order);
                self.shell.sidebar_homes.insert(id.clone(), s.workspace.clone());
            }
        }
    }
}
