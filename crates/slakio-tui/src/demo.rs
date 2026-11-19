//! The demo backend (`slakio --demo`): answers every request from the invented world of
//! `slakio-world`, at once and without any network. It is the only backend for now; the
//! binary's wiring is the one place that names it. A file downloaded is saved with invented
//! bytes ([`slakio_world::files::content`], at most [`DEMO_FILE_MAX`] of them), as the real
//! backend saves one, at once (there is nothing to cancel), in the folder the app names (the
//! binary gives the demo a temporary one).

use slakio_core::backend::{
    ACTIVITY_PAGE, Backend, Capabilities, Command, Envelope, Event, FailKind, Feed, Found, Hit, Notice, NoticeKind,
    Page, Request, SEARCH_PAGE,
};
use slakio_core::model::{Target, Ts, WorkspaceId};
use slakio_world::World;
use std::collections::VecDeque;

/// The most bytes a demo file is saved with, whatever size it says it has.
pub const DEMO_FILE_MAX: u64 = 4 * 1024 * 1024;

pub struct DemoBackend {
    world: World,
    ready: VecDeque<Envelope>,
}

impl DemoBackend {
    pub fn new(world: World) -> Self {
        Self { world, ready: VecDeque::new() }
    }
}

impl Backend for DemoBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities { demo: true, read_only: false }
    }

    fn send(&mut self, request: Request) {
        let event = match request.command {
            // One workspace's boot answers its part only.
            Command::Boot => Event::Booted(Box::new(match &request.workspace {
                Some(ws) => self.world.snapshot().only(ws),
                None => self.world.snapshot().clone(),
            })),
            Command::History { target, before, limit } => self.page(target, before, limit),
            Command::Catchup { target, limit } => self.page(target, None, limit),
            Command::Around { target, at, limit } => self.around(target, at, limit),
            Command::Dated { target, at, limit } => self.dated(target, at, limit),
            Command::Search { query, page } => {
                let ws = request.workspace.clone().unwrap_or_else(|| self.world.snapshot().workspaces[0].id.clone());
                self.search(ws, query, page)
            }
            Command::Activity { cursor } => {
                let ws = request.workspace.clone().unwrap_or_else(|| self.world.snapshot().workspaces[0].id.clone());
                self.activity(ws, cursor)
            }
            Command::Download { file, dir, max } => {
                let size = file.size.unwrap_or(0);
                let event = Event::Progress { file: file.id.clone(), got: 0, total: file.size };
                self.ready.push_back(Envelope { generation: request.id, event });
                match save(&file, &dir, max, size) {
                    Ok((done, bytes)) => {
                        Event::Downloaded { file: file.id, path: done.path, bytes, unmarked: done.unmarked }
                    }
                    Err(kind) => Event::Failed { request: request.id, kind, detail: "demo download".to_string() },
                }
            }
            // Every download is answered as it is asked for: none is still on its way.
            Command::Cancel { request } => Event::Cancelled { request, stopping: false },
            // A change that does not apply changes nothing; the answer is the list as it is.
            Command::Save { item, change } => {
                self.world.change_saved(&item, change);
                Event::Saved { items: self.world.saved().to_vec(), taken: self.world.snapshot().taken }
            }
        };
        self.ready.push_back(Envelope { generation: request.id, event });
    }

    fn poll(&mut self) -> Option<Envelope> {
        self.ready.pop_front()
    }
}

/// Save `file` (`size` bytes said) in `dir` with invented bytes, as [`slakio_core::download`]
/// says: refused when it says it is larger than `max`, or has no address.
fn save(
    file: &slakio_core::model::File,
    dir: &std::path::Path,
    max: u64,
    size: u64,
) -> Result<(slakio_core::download::Finished, u64), FailKind> {
    use slakio_core::download::{Saving, Unsaved};
    if file.download.is_none() {
        return Err(FailKind::Api { code: "file_not_downloadable".to_string() });
    }
    if size > max {
        return Err(FailKind::TooLarge);
    }
    let failed = |e: Unsaved| match e {
        Unsaved::TooLarge => FailKind::TooLarge,
        Unsaved::Io(_) => FailKind::Unsaved,
    };
    let mut saving = Saving::start(dir, &file.name, max).map_err(failed)?;
    saving.write(&slakio_world::files::content(&file.id, size, DEMO_FILE_MAX)).map_err(failed)?;
    let bytes = saving.written();
    Ok((saving.finish().map_err(failed)?, bytes))
}

impl DemoBackend {
    /// Up to `limit` messages of `target` older than `before` (the newest when `None`).
    fn page(&self, target: Target, before: Option<Ts>, limit: u32) -> Event {
        let limit = limit as usize;
        let (messages, complete) = match &target {
            Target::Conversation { conversation, .. } => self.world.history(conversation, before, limit),
            Target::Thread { conversation, thread, .. } => {
                self.world.thread(conversation, *thread, before, limit).unwrap_or((Vec::new(), true))
            }
        };
        Event::History(Box::new(Page { target, messages, complete, users: Vec::new(), cut: false }))
    }

    /// Up to `limit` messages of `target` around the first one at `at` or after: about half of
    /// them before it; the page says when its newest are not in it. A thread's from its start.
    fn dated(&self, target: Target, at: Ts, limit: u32) -> Event {
        let Target::Conversation { conversation, .. } = &target else { return self.page(target, None, limit) };
        let (count, limit) = (self.world.message_count(conversation), limit as usize);
        let first = self.world.first_at_or_after(conversation, at);
        let end = (first + limit / 2).clamp(limit.min(count), count);
        let start = end.saturating_sub(limit);
        let messages = (start..end).filter_map(|i| self.world.message(conversation, i)).collect();
        Event::History(Box::new(Page { target, messages, complete: start == 0, users: Vec::new(), cut: end < count }))
    }

    /// Page `page` of what `query` finds in `ws`, as Slack pages it.
    fn search(&self, ws: WorkspaceId, query: String, page: u32) -> Event {
        let all = self.world.search(&ws, &query);
        let per = SEARCH_PAGE as usize;
        let pages = all.len().div_ceil(per) as u32;
        let page = page.clamp(1, pages.max(1));
        let names = |id: &slakio_core::model::ConversationId| {
            let c = self.world.snapshot().conversations.iter().find(|c| c.id == *id);
            c.map(|c| c.name.clone()).unwrap_or_default()
        };
        let hits = (all.iter().skip((page as usize - 1) * per).take(per))
            .map(|h| Hit {
                conversation: h.conversation.clone(),
                name: names(&h.conversation),
                ts: h.message.ts,
                thread: h.thread,
                user: h.message.user.clone(),
                author: Default::default(),
                text: h.message.text.clone(),
                permalink: String::new(),
            })
            .collect();
        let total = all.len() as u32;
        Event::Found(Box::new(Found { workspace: ws, query, page, pages, total, hits, users: Vec::new() }))
    }

    /// The page of `ws`'s Activity after `cursor` (`c<offset>`; the newest without one).
    fn activity(&self, ws: WorkspaceId, cursor: Option<String>) -> Event {
        let all = self.world.activity(&ws);
        let from = cursor.as_deref().and_then(|c| c.strip_prefix('c')?.parse::<usize>().ok()).unwrap_or(0);
        let per = ACTIVITY_PAGE as usize;
        let notices = (all.iter().skip(from).take(per))
            .map(|n| Notice {
                key: n.key.clone(),
                kind: match n.kind {
                    "at_channel" => NoticeKind::Everyone,
                    "at_user_group" => NoticeKind::Group,
                    "keyword" => NoticeKind::Keyword,
                    "thread_v2" => NoticeKind::Thread,
                    "message_reaction" => NoticeKind::Reaction,
                    "dm" => NoticeKind::Dm,
                    _ => NoticeKind::Mention,
                },
                at: n.at,
                conversation: n.conversation.clone(),
                ts: n.message.ts,
                thread: n.thread,
                user: n.reactor.as_ref().map_or_else(|| n.message.user.clone(), |(u, _)| u.clone()),
                text: Some(n.message.text.clone()),
                reaction: n.reactor.as_ref().map(|(_, r)| (*r).into()),
                unread: n.unread,
            })
            .collect();
        let next = (from + per < all.len()).then(|| format!("c{}", from + per));
        Event::Activity(Box::new(Feed { workspace: ws, after: cursor, notices, next, users: Vec::new() }))
    }

    /// Up to `limit` messages of `target` up to the one at `at`, and the newest `limit` after it
    /// when they reach back to it (as Slack's are read); else the page says its newest are not
    /// in it. A thread's from its start.
    fn around(&self, target: Target, at: Ts, limit: u32) -> Event {
        let Target::Conversation { conversation, .. } = &target else { return self.page(target, None, limit) };
        let (mut messages, complete) = self.world.history(conversation, Some(Ts(at.0 + 1)), limit as usize);
        let newest = self.world.history(conversation, None, limit as usize).0;
        let reaches = newest.first().is_some_and(|m| m.ts <= at) || newest.last().is_none_or(|m| m.ts <= at);
        if reaches {
            messages.extend(newest.into_iter().filter(|m| m.ts > at));
        }
        Event::History(Box::new(Page { target, messages, complete, users: Vec::new(), cut: !reaches }))
    }
}
