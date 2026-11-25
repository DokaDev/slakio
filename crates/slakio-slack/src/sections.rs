//! The sidebar's sections (`users.channelSections.list`), read a page at a time. Slack's answer
//! carries a top-level `cursor` on every page, and it is not where a next page starts: only
//! `response_metadata.next_cursor` is. A next page is asked for only while that names one and
//! the sections read so far leave some unread: their `next_channel_section_id` chain names one
//! not read yet, fewer than the answer's `count` were read, and the last page brought a new one.
//! The sections are read at each boot, for the sidebar ([`crate::sidebar`]).

use crate::client::Client;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use crate::sidebar::ChannelSection;
use serde_json::Value;
use slakio_core::diag::{Kind, Stage};
use std::collections::HashSet;

/// The most pages read.
pub const PAGES_MAX: usize = 5;

/// The sections of a page.
pub(crate) fn list(v: &Value) -> Option<&Vec<Value>> {
    ["channel_sections", "sections"].into_iter().find_map(|k| v.get(k).and_then(Value::as_array))
}

/// A non-empty string field of `v`.
fn text<'a>(v: &'a Value, name: &str) -> Option<&'a str> {
    v.get(name).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// What the pages read so far hold: their sections' ids, and the ids those name as next.
#[derive(Debug, Default)]
pub(crate) struct Paging {
    seen: HashSet<String>,
    named: HashSet<String>,
}

/// After a page: the cursor of the next one to ask for, or why there is none.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Next {
    Page(String),
    Done(&'static str),
}

impl Paging {
    /// Take in `page`, and say whether another is to be asked for.
    pub(crate) fn next(&mut self, page: &Value) -> Next {
        let before = self.seen.len();
        for s in list(page).into_iter().flatten() {
            if let Some(id) = text(s, "channel_section_id") {
                self.seen.insert(id.to_string());
            }
            if let Some(next) = text(s, "next_channel_section_id") {
                self.named.insert(next.to_string());
            }
        }
        let count = page.get("count").and_then(Value::as_u64);
        if self.seen.len() == before {
            return Next::Done("no-new-section");
        }
        if count.is_some_and(|c| self.seen.len() as u64 >= c) {
            return Next::Done("count-reached");
        }
        if self.named.is_subset(&self.seen) {
            return Next::Done("chain-closed");
        }
        match page.pointer("/response_metadata/next_cursor").and_then(Value::as_str).filter(|c| !c.is_empty()) {
            Some(c) => Next::Page(c.to_string()),
            None => Next::Done("no-cursor"),
        }
    }
}

/// The sidebar's sections of the workspace (read-only, in the background share), a page at a
/// time, at most [`PAGES_MAX`] pages; `None` when the first page could not be read (a later
/// page that fails ends the list there). Slack refusing it never ends the sign-in: the sidebar
/// does without. With the diagnostics on, each page's shape is told.
pub(crate) async fn fetch(client: &Client) -> Option<Vec<ChannelSection>> {
    let diag = client.diag();
    let (mut paging, mut cursor, mut out) = (Paging::default(), None::<String>, None::<Vec<ChannelSection>>);
    for page in 1..=PAGES_MAX {
        let n = page.to_string();
        let params: Vec<(&str, &str)> = cursor.as_deref().map(|c| ("cursor", c)).into_iter().collect();
        let asked = if cursor.is_some() { "yes" } else { "no" };
        diag.line(Kind::Sections, Stage::Asked, "ok", &[("page", &n), ("cursor", asked)]);
        let call = Call::form(Method::UsersChannelSectionsList, &params).probe();
        let v = match client.call::<Value>(&call, Priority::Background).await {
            Ok(v) => v,
            Err(e) => {
                crate::shape::failed(diag, Kind::Sections, &e, &[("page", &n)]);
                return out;
            }
        };
        if diag.on() {
            crate::shape::section_shape(diag, &v, &n, cursor.as_deref());
        }
        // A page without its list: the shape above says so (`sections failed parse`).
        let Some(list) = list(&v) else { return out };
        let read = list.iter().filter_map(|s| serde_json::from_value::<ChannelSection>(s.clone()).ok());
        out.get_or_insert_with(Vec::new).extend(read);
        match paging.next(&v) {
            Next::Page(c) => cursor = Some(c),
            Next::Done(why) => {
                diag.line(Kind::Sections, Stage::Counted, "done", &[("pages", &n), ("why", why)]);
                return out;
            }
        }
    }
    diag.line(Kind::Sections, Stage::Counted, "page-cap", &[("pages", &PAGES_MAX.to_string())]);
    out
}

#[cfg(test)]
mod tests;
