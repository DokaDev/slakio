//! Searching a workspace's messages (`search.messages`), a page at a time and only when the
//! user asks: the query goes to Slack as typed (its own words: `in:#channel`, `from:@name`,
//! quotes), newest first, the words matched marked as Slack marks them. A reply found is told
//! apart by its web link, which names the thread it is in.
//!
//! Derived from gammons/slk (MIT, Copyright (c) 2026 Grant Ammons): the query passed through
//! verbatim to `search.messages`, and a reply's thread recovered from its permalink.

use crate::adapter::Session;
use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use crate::wire;
use slakio_core::backend::{Found, Hit, SEARCH_PAGE};
use slakio_core::model::{ConversationId, Ts, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;

/// The last page Slack gives.
pub const SEARCH_PAGES_MAX: u32 = 100;

/// The most of a query sent (in characters); the rest is left out.
pub const QUERY_MAX: usize = 500;

// derived from gammons/slk (MIT)
/// Page `page` of the messages `query` finds in the workspace, in the user's share of the
/// request budget, with their authors by name (looked up as a page's are).
pub(crate) async fn search(client: &Client, ctx: &Session, query: &str, page: u32) -> Result<Found, SlackError> {
    let workspace = WorkspaceId::new(ctx.people.team().unwrap_or_default());
    let query: String = query.trim().chars().take(QUERY_MAX).collect();
    let page = page.clamp(1, SEARCH_PAGES_MAX);
    if query.is_empty() {
        return Ok(Found { workspace, query, page, pages: 0, total: 0, hits: Vec::new(), users: Vec::new() });
    }
    let (count, number) = (SEARCH_PAGE.to_string(), page.to_string());
    let params = [
        ("query", query.as_str()),
        ("count", count.as_str()),
        ("page", number.as_str()),
        ("highlight", "true"),
        ("sort", "timestamp"),
        ("sort_dir", "desc"),
    ];
    let answer: wire::Search = client.call(&Call::form(Method::SearchMessages, &params), Priority::User).await?;
    let found = answer.messages.unwrap_or_default();
    let paging = found.paging.unwrap_or_default();
    let hits: Vec<Hit> = found.matches.unwrap_or_default().into_iter().filter_map(hit).collect();
    let authors: Vec<String> = hits.iter().map(|h| h.user.as_str().to_string()).filter(|u| !u.is_empty()).collect();
    let users = ctx.people.names(client, &authors, Priority::User).await;
    Ok(Found {
        workspace,
        query,
        page: paging.page.unwrap_or(page),
        pages: paging.pages.unwrap_or(u32::from(!hits.is_empty())).min(SEARCH_PAGES_MAX),
        total: found.total.or(paging.total).unwrap_or(hits.len() as u32),
        hits,
        users,
    })
}

/// A match as a hit; `None` without a conversation or a time.
fn hit(m: wire::SearchMatch) -> Option<Hit> {
    let ts = Ts(wire::ts(&m.ts)?);
    let channel = m.channel?;
    let permalink = m.permalink.unwrap_or_default();
    let thread = thread_of(&permalink, &channel.id, ts);
    Some(Hit {
        conversation: ConversationId::new(&channel.id),
        name: Remote::new(channel.name.unwrap_or_default()),
        ts,
        thread,
        user: UserId::new(m.user.unwrap_or_default()),
        author: Remote::new(m.username.unwrap_or_default()),
        text: Remote::new(m.text.unwrap_or_default()),
        permalink,
    })
}

// derived from gammons/slk (MIT)
/// The thread a message found at `ts` in `channel` is a reply in, by its web link's
/// `thread_ts`: only when the link names no other conversation (`cid`), and never the message
/// itself.
pub(crate) fn thread_of(permalink: &str, channel: &str, ts: Ts) -> Option<Ts> {
    let (_, query) = permalink.split_once('?')?;
    let query = query.split('#').next().unwrap_or_default();
    let (mut thread, mut cid) = (None, None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("thread_ts", v)) => thread = Some(wire::ts(v).map(Ts)?),
            Some(("cid", v)) => cid = Some(v),
            _ => {}
        }
    }
    if cid.is_some_and(|c| c != channel) {
        return None;
    }
    thread.filter(|t| *t != ts)
}

#[cfg(test)]
mod tests;
