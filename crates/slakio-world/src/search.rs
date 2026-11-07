//! Searching the invented world's messages as Slack's search does, roughly: words (a quoted run
//! is one) found in a message's text, any case; `in:#name` keeps one conversation, `from:@name`
//! one person; other `word:value` filters are left out. Newest first, each word found marked as
//! Slack marks it (two Private Use Area characters around it). Replies are searched too, but
//! for the big history's (too many to go through each time).

use crate::{World, names};
use slakio_core::backend::{MATCH_END, MATCH_START};
use slakio_core::model::{ConversationId, Message, Ts, WorkspaceId};

/// A message the world's search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub conversation: ConversationId,
    /// The words found marked in its text.
    pub message: Message,
    /// The message it is a reply under.
    pub thread: Option<Ts>,
}

/// A query read: its words, and its filters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub words: Vec<String>,
    pub within: Option<String>,
    pub from: Option<String>,
}

/// `query` read (see the module's text).
pub fn parse(query: &str) -> Query {
    let mut q = Query::default();
    let mut rest = query.trim();
    while !rest.is_empty() {
        let (token, quoted, tail) = match rest.strip_prefix('"') {
            Some(after) => {
                let end = after.find('"').unwrap_or(after.len());
                (&after[..end], true, after.get(end + 1..).unwrap_or_default())
            }
            None => {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                (&rest[..end], false, &rest[end..])
            }
        };
        rest = tail.trim_start();
        let lower = token.to_ascii_lowercase();
        match lower.split_once(':').filter(|_| !quoted) {
            Some(("in", v)) => q.within = Some(v.trim_start_matches('#').to_string()),
            Some(("from", v)) => q.from = Some(v.trim_start_matches('@').to_string()),
            Some(_) => {}
            None if !lower.is_empty() => q.words.push(lower),
            None => {}
        }
    }
    q
}

/// `text` with every word of `words` found in it (any ASCII case) marked.
fn mark(text: &str, words: &[String]) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for w in words {
        let found: Vec<(usize, usize)> = lower.match_indices(w.as_str()).map(|(i, m)| (i, i + m.len())).collect();
        if found.is_empty() {
            return None;
        }
        spans.extend(found);
    }
    spans.sort_unstable();
    let mut out = String::with_capacity(text.len() + spans.len() * 6);
    let mut at = 0;
    for (start, end) in spans {
        if start < at {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push(MATCH_START);
        out.push_str(&text[start..end]);
        out.push(MATCH_END);
        at = end;
    }
    out.push_str(&text[at..]);
    Some(out)
}

impl World {
    /// The messages of `workspace` that `query` finds, newest first (see the module's text).
    pub fn search(&self, workspace: &WorkspaceId, query: &str) -> Vec<SearchHit> {
        let q = parse(query);
        let s = self.snapshot();
        let from = q.from.as_ref().map(|f| {
            s.users.iter().filter(|u| u.workspace == *workspace && u.name == f.as_str()).map(|u| u.id.clone()).collect()
        });
        let mut hits = Vec::new();
        for c in s.conversations.iter().filter(|c| c.workspace == *workspace) {
            if q.within.as_ref().is_some_and(|w| c.name != w.as_str()) {
                continue;
            }
            let found = |m: Message, thread: Option<Ts>| {
                if from.as_ref().is_some_and(|ids: &Vec<_>| !ids.contains(&m.user)) {
                    return None;
                }
                let text = mark(m.text.unsanitized(), &q.words)?;
                Some(SearchHit { conversation: c.id.clone(), message: Message { text: text.into(), ..m }, thread })
            };
            for i in 0..self.message_count(&c.id) {
                let Some(m) = self.message(&c.id, i) else { continue };
                let replies = m.thread.map_or(0, |t| t.replies);
                let parent = m.ts;
                hits.extend(found(m, None));
                if c.name == names::BIG_HISTORY {
                    continue;
                }
                for r in 0..replies {
                    hits.extend(self.reply(&c.id, i, r).and_then(|m| found(m, Some(parent))));
                }
            }
        }
        if q.words.is_empty() && q.within.is_none() && q.from.is_none() {
            hits.clear();
        }
        hits.sort_by(|a, b| b.message.ts.cmp(&a.message.ts));
        hits
    }
}

#[cfg(test)]
mod tests;
