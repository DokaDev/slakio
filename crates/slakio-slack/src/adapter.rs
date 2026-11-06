//! The Slack backend behind a [`BackendHandle`]: a task that owns the [`BackendEnd`], reads the
//! UI's requests and answers each one exactly once (its event, or `Failed`), never with
//! silence. A change is refused before anything is sent (the backend is read-only). A newer
//! page of a target (`History`, `Around`) supersedes the one of the same target still on its
//! way: that one is cancelled — before it is sent, or by dropping its answer — and answered
//! `Failed(Busy)`. Pages of other targets never cancel each other: the panes of a layout opened
//! again ask for theirs at once, and each gets its own. They all go through the governor, which
//! paces them. When the UI is gone the task ends, and what it had in flight with it.
//!
//! Opening a conversation (its newest page, `conversations.view`) waits [`OPEN_DEBOUNCE`]
//! first: opening it again meanwhile fetches it once.
//!
//! A thread is read from its start by `conversations.replies` (Slack pages it forward only) and
//! kept for the session ([`crate::threads`]): its newest page is the end of what was read, its
//! older pages come from what is kept, without a request.

use crate::boot::{Me, boot};
use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::messages::{Bots, bot, read};
use crate::method::Method;
use crate::people::People;
use crate::request::Call;
use crate::threads::Threads;
use crate::wire;
use slakio_core::backend::{
    BackendEnd, BackendHandle, Capabilities, Command, Envelope, Event, Generation, Page, Request, refuse_change,
};
use slakio_core::model::{ConversationId, Message, Target, Ts, UserId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinSet};

/// How many requests, and how many events, wait in the handle's channels at most.
pub const BOUND: usize = 64;

/// The most messages one page asks for: what the web client shows first.
pub const PAGE_MAX: u32 = 28;

/// How long an open waits before its conversation is fetched: opening it again meanwhile
/// fetches it once.
pub const OPEN_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(200);

/// What the backend can do: read, never change.
pub const CAPABILITIES: Capabilities = Capabilities { demo: false, read_only: true };

/// Run the backend of `client` on a task of its own (inside a Tokio runtime), without live
/// updates: it answers requests only.
pub fn serve(client: Client) -> BackendHandle {
    let (handle, end) = BackendHandle::pair(CAPABILITIES, BOUND);
    tokio::spawn(run(Arc::new(client), end, Arc::default()));
    handle
}

/// [`serve`], with the workspace's live updates pushed as well once it has booted
/// ([`crate::live`]), connecting as `realtime` says.
pub fn serve_live(client: Client, realtime: crate::live::Realtime) -> BackendHandle {
    let (handle, end) = BackendHandle::pair(CAPABILITIES, BOUND);
    let (client, session) = (Arc::new(client), Arc::<Session>::default());
    let live = tokio::spawn(crate::live::run(Arc::clone(&client), Arc::clone(&session), end.tx.clone(), realtime));
    // Once the UI drops the handle (or puts another sign-in's in its place), the socket goes too.
    tokio::spawn(async move {
        run(client, end, session).await;
        live.abort();
    });
    handle
}

/// [`serve_live`] connecting as `realtime` says, or [`serve`] without it (live updates off).
pub fn serve_with(client: Client, realtime: Option<crate::live::Realtime>) -> BackendHandle {
    match realtime {
        Some(realtime) => serve_live(client, realtime),
        None => serve(client),
    }
}

/// What the backend learns in a session: the user's own id once booted (whose reactions are
/// "mine"), the people named so far, the threads read.
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) me: Me,
    pub(crate) people: People,
    pub(crate) threads: Threads,
    /// Told each time a boot succeeded (the live updates wait for the first).
    pub(crate) booted: tokio::sync::Notify,
    /// The conversations the UI read this session: the live updates name new authors only there.
    pub(crate) opened: std::sync::Mutex<std::collections::HashSet<ConversationId>>,
}

pub use crate::people::LOOKUP_BATCH;

/// A page on its way: its request, its task, and whether it was answered (by the task, or as
/// superseded): whichever comes first answers, the other never does.
struct InFlight {
    id: Generation,
    task: AbortHandle,
    answered: Arc<AtomicBool>,
}

/// The page of `target` on its way, superseded by a newer one: cancelled and answered
/// `Failed(Busy)`, unless it was answered already. `false` when the UI is gone.
async fn supersede(in_flight: &mut HashMap<Target, InFlight>, target: &Target, tx: &mpsc::Sender<Envelope>) -> bool {
    let Some(older) = in_flight.remove(target) else { return true };
    if older.answered.swap(true, Ordering::SeqCst) {
        return true;
    }
    older.task.abort();
    let e = SlackError::Superseded;
    tx.send(Envelope::failed(older.id, e.kind(), e.detail())).await.is_ok()
}

async fn run(client: Arc<Client>, mut end: BackendEnd, session: Arc<Session>) {
    let mut tasks = JoinSet::new();
    let mut in_flight: HashMap<Target, InFlight> = HashMap::new();
    while let Some(request) = end.rx.recv().await {
        while tasks.try_join_next().is_some() {}
        if let Some(refused) = refuse_change(&request) {
            if end.tx.send(refused).await.is_err() {
                return;
            }
            continue;
        }
        let (client, tx, ctx) = (Arc::clone(&client), end.tx.clone(), Arc::clone(&session));
        let Request { id, command, .. } = request;
        if let Command::History { target, .. } | Command::Catchup { target, .. } | Command::Around { target, .. } =
            &command
        {
            session.opened.lock().unwrap_or_else(PoisonError::into_inner).insert(target.conversation().clone());
        }
        match command {
            Command::Boot => {
                tasks.spawn(async move {
                    let booted = boot(&client, &ctx.me, &ctx.people).await;
                    if booted.is_ok() {
                        ctx.booted.notify_one();
                    }
                    answer(&tx, id, booted).await;
                });
            }
            Command::Catchup { target, limit } => {
                tasks.spawn(async move {
                    let page = catch_up(&client, &ctx, target, limit).await;
                    answer(&tx, id, page.map(|p| Event::History(Box::new(p)))).await;
                });
            }
            Command::History { target, before, limit } => {
                if !supersede(&mut in_flight, &target, &end.tx).await {
                    return;
                }
                let (key, answered) = (target.clone(), Arc::new(AtomicBool::new(false)));
                let once = Arc::clone(&answered);
                let opening = before.is_none() && matches!(target, Target::Conversation { .. });
                let task = tasks.spawn(async move {
                    if opening {
                        tokio::time::sleep(OPEN_DEBOUNCE).await;
                    }
                    let page = history(&client, &ctx, target, before, limit).await;
                    if !once.swap(true, Ordering::SeqCst) {
                        answer(&tx, id, page.map(|p| Event::History(Box::new(p)))).await;
                    }
                });
                in_flight.insert(key, InFlight { id, task, answered });
            }
            // A link opened: a request the user waits for, like an open, but never debounced.
            Command::Around { target, at, limit } => {
                if !supersede(&mut in_flight, &target, &end.tx).await {
                    return;
                }
                let (key, answered) = (target.clone(), Arc::new(AtomicBool::new(false)));
                let once = Arc::clone(&answered);
                let task = tasks.spawn(async move {
                    let page = around(&client, &ctx, target, at, limit).await;
                    if !once.swap(true, Ordering::SeqCst) {
                        answer(&tx, id, page.map(|p| Event::History(Box::new(p)))).await;
                    }
                });
                in_flight.insert(key, InFlight { id, task, answered });
            }
            // A change never gets here (refused above); answered all the same.
            Command::Save { .. } => {
                let refused = Envelope::failed(id, slakio_core::backend::FailKind::ReadOnly, "");
                if end.tx.send(refused).await.is_err() {
                    return;
                }
            }
        }
        in_flight.retain(|_, f| !f.task.is_finished());
    }
}

/// Send the answer to request `id`.
async fn answer(tx: &mpsc::Sender<Envelope>, id: Generation, result: Result<Event, SlackError>) {
    let envelope = match result {
        Ok(event) => Envelope { generation: id, event },
        Err(e) => Envelope::failed(id, e.kind(), e.detail()),
    };
    // A closed channel: the UI is gone, nobody waits.
    let _ = tx.send(envelope).await;
}

/// A page of `target`: the newest through `conversations.view`, older ones through
/// `conversations.history`, a thread's from what [`Threads`] read of it (`conversations.replies`).
async fn history(
    client: &Client,
    ctx: &Session,
    target: Target,
    before: Option<Ts>,
    limit: u32,
) -> Result<Page, SlackError> {
    // An older page is reading back, which nobody waits for the way they wait for what they
    // opened: it takes the background share of the request budget (the user's stays free). A
    // thread's older page comes from what was kept, so it may be larger (no request).
    let priority = if before.is_some() { Priority::Background } else { Priority::User };
    let kept_thread = before.is_some() && matches!(target, Target::Thread { .. });
    let limit = limit.clamp(1, if kept_thread { crate::threads::BATCH } else { PAGE_MAX });
    let channel = target.conversation().as_str().to_string();
    let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (messages, complete, bots, cut) = match (&target, before) {
        (Target::Thread { thread, .. }, _) => {
            ctx.threads.page(client, me.as_ref(), (&channel, *thread), before, limit, Priority::User).await?
        }
        (Target::Conversation { .. }, None) => {
            let count = limit.to_string();
            let call = Call::form(
                Method::ConversationsView,
                &[
                    ("channel", channel.as_str()),
                    ("count", count.as_str()),
                    ("ignore_replies", "true"),
                    ("no_members", "true"),
                ],
            );
            let view: wire::View = client.call(&call, Priority::User).await?;
            ctx.people.learn(view.users.as_deref().unwrap_or_default());
            let history = view.history.unwrap_or_default();
            // Unknown is not complete: only Slack's own "no more" says the start is in this page.
            let complete = history.has_more == Some(false);
            let (messages, bots) = read(history.messages.unwrap_or_default(), me.as_ref());
            (messages, complete, bots, false)
        }
        (Target::Conversation { .. }, Some(before)) => {
            let (messages, complete, bots) = older(client, me.as_ref(), &channel, before, limit, priority).await?;
            (messages, complete, bots, false)
        }
    };
    // A thread's authors are all looked up when it is read, so its older pages, served from what
    // was kept, ask for nothing.
    let everyone: Option<Vec<String>> = match (&target, before) {
        (Target::Thread { thread, .. }, None) => Some(ctx.threads.authors(&channel, *thread)),
        _ => None,
    };
    let page = Page { target, messages, complete, users: Vec::new(), cut };
    Ok(named(client, ctx, page, everyone, bots, me.as_ref(), priority).await)
}

/// `page` with its authors by name (`everyone`'s, when given, else its messages'): what the view
/// brought, else looked up (in batches); not the user, nor a bot (named by the name it posts as).
async fn named(
    client: &Client,
    ctx: &Session,
    mut page: Page,
    everyone: Option<Vec<String>>,
    bots: Bots,
    me: Option<&UserId>,
    priority: Priority,
) -> Page {
    let everyone = everyone.unwrap_or_else(|| page.messages.iter().map(|m| m.user.as_str().to_string()).collect());
    let authors: Vec<String> = everyone
        .into_iter()
        .filter(|u| !u.is_empty() && me.is_none_or(|me| me.as_str() != u) && !bots.contains_key(u))
        .collect();
    page.users = ctx.people.names(client, &authors, priority).await;
    if let Some(ws) = ctx.people.team() {
        page.users.extend(bots.into_iter().map(|(id, b)| bot(&ws, id, b)));
    }
    page
}

/// The newest `limit` messages of `target` read again after live updates were lost, in the
/// background share of the request budget, so it never waits for, nor cancels, an open: a
/// conversation's by `conversations.history` with no bound; a thread's by reading what is new in
/// it ([`Threads`]: one request mostly, the whole thread again when it fits in one).
async fn catch_up(client: &Client, ctx: &Session, target: Target, limit: u32) -> Result<Page, SlackError> {
    let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let conversation = match &target {
        Target::Conversation { conversation, .. } => conversation,
        Target::Thread { conversation, thread, .. } => {
            let (channel, limit) = (conversation.as_str(), limit.clamp(1, PAGE_MAX));
            let (messages, complete, bots, cut) =
                ctx.threads.page(client, me.as_ref(), (channel, *thread), None, limit, Priority::Background).await?;
            let everyone = Some(ctx.threads.authors(channel, *thread));
            let page = Page { target: target.clone(), messages, complete, users: Vec::new(), cut };
            return Ok(named(client, ctx, page, everyone, bots, me.as_ref(), Priority::Background).await);
        }
    };
    let count = limit.clamp(1, PAGE_MAX).to_string();
    let params = [("channel", conversation.as_str()), ("limit", count.as_str()), ("ignore_replies", "true")];
    let call = Call::form(Method::ConversationsHistory, &params);
    let history: wire::History = client.call(&call, Priority::Background).await?;
    // Unknown is not complete: only Slack's own "no more" says the start is in this page.
    let complete = history.has_more == Some(false);
    let (messages, bots) = read(history.messages.unwrap_or_default(), me.as_ref());
    let page = Page { target, messages, complete, users: Vec::new(), cut: false };
    Ok(named(client, ctx, page, None, bots, me.as_ref(), Priority::Background).await)
}

/// The messages of `target` around the one at `at`, oldest first, in the user's share of the
/// request budget: a conversation's by two `conversations.history` requests, the message and
/// those before it (`latest`, `inclusive`), then those after it (`oldest`). Slack lists the
/// newest first, so the newer ones join only when Slack says none is left past them: then the
/// page reaches the newest; else they are left out and the page says its newest messages are
/// not read ([`Page::cut`]), so no gap is ever hidden; so too when the second request fails. A
/// thread is read as when it opens.
async fn around(client: &Client, ctx: &Session, target: Target, at: Ts, limit: u32) -> Result<Page, SlackError> {
    let Target::Conversation { conversation, .. } = &target else {
        return history(client, ctx, target, None, limit).await;
    };
    let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (channel, count, at_text) = (conversation.as_str(), limit.clamp(1, PAGE_MAX).to_string(), at.to_string());
    let older = [
        ("channel", channel),
        ("latest", at_text.as_str()),
        ("inclusive", "true"),
        ("limit", count.as_str()),
        ("ignore_replies", "true"),
    ];
    let older: wire::History = client.call(&Call::form(Method::ConversationsHistory, &older), Priority::User).await?;
    let newer =
        [("channel", channel), ("oldest", at_text.as_str()), ("limit", count.as_str()), ("ignore_replies", "true")];
    // The newer ones not read (but for the sign-in, which every request would fail with): the
    // message and those before it are shown all the same, as a page whose newest are not read.
    let newer =
        match client.call::<wire::History>(&Call::form(Method::ConversationsHistory, &newer), Priority::User).await {
            Err(e @ SlackError::Auth { .. }) => return Err(e),
            newer => newer.ok(),
        };
    // Unknown is not complete: only Slack's own "no more" says the start (or the newest) is in.
    let complete = older.has_more == Some(false);
    let reaches = newer.as_ref().is_some_and(|n| n.has_more == Some(false));
    let mut raw = older.messages.unwrap_or_default();
    if reaches {
        raw.extend(newer.and_then(|n| n.messages).unwrap_or_default());
    }
    let (mut messages, bots) = read(raw, me.as_ref());
    // `read` puts them in order, each once.
    messages.retain(|m| m.ts <= at || reaches);
    let page = Page { target, messages, complete, users: Vec::new(), cut: !reaches };
    Ok(named(client, ctx, page, None, bots, me.as_ref(), Priority::User).await)
}

/// Pages of deleted messages only read past at most, in one older page.
pub const SKIPPED_MAX: usize = 4;

/// Up to `limit` messages of `channel` older than `before`, oldest first; whether its start is
/// among them; the bots that posted. A page whose messages are all deleted is passed over (up to
/// [`SKIPPED_MAX`] of them): the next older one is read, from the oldest of them on.
async fn older(
    client: &Client,
    me: Option<&UserId>,
    channel: &str,
    before: Ts,
    limit: u32,
    priority: Priority,
) -> Result<(Vec<Message>, bool, Bots), SlackError> {
    let count = limit.to_string();
    let mut latest = before;
    for skipped in 0..=SKIPPED_MAX {
        let at = latest.to_string();
        let params =
            [("channel", channel), ("latest", at.as_str()), ("limit", count.as_str()), ("ignore_replies", "true")];
        let history: wire::History = client.call(&Call::form(Method::ConversationsHistory, &params), priority).await?;
        let has_more = history.has_more;
        let raw = history.messages.unwrap_or_default();
        let oldest = raw.iter().filter_map(|m| wire::ts(&m.ts)).map(Ts).filter(|t| *t < latest).min();
        let (mut messages, bots) = read(raw, me);
        messages.retain(|m| m.ts < before);
        match oldest {
            Some(o) if messages.is_empty() && has_more == Some(true) && skipped < SKIPPED_MAX => latest = o,
            // Unknown is not complete: only Slack's own "no more" says the start is in this
            // page; but an older page with nothing in it has nothing older to give either
            // (asking again would ask for the same).
            _ => {
                let complete = has_more == Some(false) || messages.is_empty();
                return Ok((messages, complete, bots));
            }
        }
    }
    Ok((Vec::new(), true, Bots::new()))
}
