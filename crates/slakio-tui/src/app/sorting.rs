//! How the sidebar's sections order their conversations: the settings (`channel_sort`,
//! `dm_sort`) and the user's choice for one section (`:sort-section`, which switches the section
//! under the list's cursor between by name and the newest first).

use super::App;
use super::model::{Row, Sorting};
use slakio_core::config::Config;
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::Sort;
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
        self.info(Msg::SectionSortChanged { name: next.name().to_string() }, now);
    }
}
