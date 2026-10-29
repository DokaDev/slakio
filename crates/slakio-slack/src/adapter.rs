//! The Slack backend behind a [`BackendHandle`]: a task that owns the [`BackendEnd`], reads the
//! UI's requests and answers each one exactly once (its event, or `Failed`), never with
//! silence. A change is refused before anything is sent (the backend is read-only). A newer
//! `History` for the same target supersedes one still in flight: the older one is dropped
//! unanswered, since the UI no longer waits for it. When the UI is gone the task ends, and
//! what it had in flight with it.
//!
//! Opening a conversation (its newest page, `conversations.view`) waits [`OPEN_DEBOUNCE`]
//! first, and at most one is under way: a newer open cancels the older one — before it is
//! sent, or by dropping its answer — and the older one is answered `Failed(Busy)`, so a pane
//! that still shows it can be retried instead of waiting.
//!
//! A thread is read from its start by `conversations.replies` (Slack pages it forward only) and
//! kept for the session ([`crate::threads`]): its newest page is the end of what was read, its
//! older pages come from what is kept, without a request.

use crate::boot::{Me, boot};
use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::people::People;
use crate::request::Call;
use crate::threads::Threads;
use crate::wire;
use slakio_core::backend::{
    BackendEnd, BackendHandle, Capabilities, Command, Envelope, Event, Generation, Page, Request, refuse_change,
};
use slakio_core::model::{
    ConversationId, Message, Org, Presence, Reaction, Target, ThreadSummary, Ts, User, UserId, WorkspaceId,
};
use slakio_core::sanitize::Remote;
use std::collections::HashMap;
use std::sync::{Arc, PoisonError};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{AbortHandle, JoinSet};

/// How many requests, and how many events, wait in the handle's channels at most.
pub const BOUND: usize = 64;

/// The most messages one page asks for: what the web client shows first.
pub const PAGE_MAX: u32 = 28;

/// How long an open waits before its conversation is fetched: moving quickly through
/// conversations fetches only the one the user stops on.
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
    tokio::spawn(crate::live::run(Arc::clone(&client), Arc::clone(&session), end.tx.clone(), realtime));
    tokio::spawn(run(client, end, session));
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

async fn run(client: Arc<Client>, mut end: BackendEnd, session: Arc<Session>) {
    let mut tasks = JoinSet::new();
    let mut in_flight: HashMap<Target, AbortHandle> = HashMap::new();
    // The open under way, to cancel when a newer one comes.
    let mut open: Option<oneshot::Sender<()>> = None;
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
        if let Command::History { target, .. } | Command::Catchup { target, .. } = &command {
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
                if let Some(older) = in_flight.remove(&target) {
                    older.abort();
                }
                let key = target.clone();
                let opening = before.is_none() && matches!(target, Target::Conversation { .. });
                let cancelled = opening.then(|| {
                    if let Some(older) = open.take() {
                        let _ = older.send(());
                    }
                    let (cancel, cancelled) = oneshot::channel();
                    open = Some(cancel);
                    cancelled
                });
                let task = tasks.spawn(async move {
                    let fetch = async {
                        if opening {
                            tokio::time::sleep(OPEN_DEBOUNCE).await;
                        }
                        history(&client, &ctx, target, before, limit).await
                    };
                    let page = match cancelled {
                        Some(cancelled) => tokio::select! {
                            biased;
                            _ = cancelled => Err(SlackError::Superseded),
                            page = fetch => page,
                        },
                        None => fetch.await,
                    };
                    answer(&tx, id, page.map(|p| Event::History(Box::new(p)))).await;
                });
                in_flight.insert(key, task);
            }
            // A change never gets here (refused above); answered all the same.
            Command::Save { .. } => {
                let refused = Envelope::failed(id, slakio_core::backend::FailKind::ReadOnly, "");
                if end.tx.send(refused).await.is_err() {
                    return;
                }
            }
        }
        in_flight.retain(|_, task| !task.is_finished());
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
            ctx.threads.page(client, me.as_ref(), &channel, *thread, before, limit).await?
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

/// The newest `limit` messages of conversation `target` read again after live updates were lost:
/// `conversations.history` with no bound, in the background share of the request budget, so it
/// never waits for, nor cancels, an open.
async fn catch_up(client: &Client, ctx: &Session, target: Target, limit: u32) -> Result<Page, SlackError> {
    let Target::Conversation { conversation, .. } = &target else {
        return Err(SlackError::Api { code: "thread_not_caught_up".to_string() });
    };
    let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
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

/// A bot that posted: the name it posted as, and its team when Slack gave one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bot {
    pub name: String,
    pub team: Option<String>,
}

/// The bots that posted in a page, by the author id their messages carry: one for each bot and
/// name it posted as, so a bot that posts under another name later does not rename its earlier
/// messages.
pub(crate) type Bots = HashMap<String, Bot>;

/// The author id of the messages bot `id` posted as `name`: the bot's id and a hash of the name
/// (no character of the name in it).
fn bot_author(id: &str, name: &str) -> String {
    // FNV-1a: the same id every run (the cache keeps it).
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{id}~{hash:016x}")
}

/// `raw` as messages, oldest first, each once (of a message Slack lists twice, the copy edited
/// last), the deleted ones left out; and the bots among their authors.
pub(crate) fn read(raw: Vec<wire::Message>, me: Option<&UserId>) -> (Vec<Message>, Bots) {
    let mut bots = Bots::new();
    let mut messages: Vec<(Message, u64)> =
        raw.into_iter()
            .filter_map(|mut m| {
                if let (None, Some(id), Some(name)) = (&m.user, &m.bot_id, &m.username) {
                    let author = bot_author(id, name);
                    let team = m.bot_profile.as_ref().and_then(|b| b.team_id.clone()).or_else(|| m.team.clone());
                    bots.insert(author.clone(), Bot { name: name.clone(), team });
                    m.user = Some(author);
                }
                // When it was edited last: 0 never, 1 at a time Slack did not give.
                let edited = m.edited.as_ref().map_or(0, |e| {
                    e.get("ts").and_then(serde_json::Value::as_str).and_then(wire::ts).unwrap_or(1).max(1)
                });
                message(m, me).map(|m| (m, edited))
            })
            .collect();
    messages.sort_by_key(|(m, _)| m.ts);
    messages.dedup_by(|later, kept| {
        if later.0.ts != kept.0.ts {
            return false;
        }
        if later.1 > kept.1 {
            std::mem::swap(later, kept);
        }
        true
    });
    (messages.into_iter().map(|(m, _)| m).collect(), bots)
}

/// A bot of workspace `ws` as a person: named as it posts; of another organization when its
/// team is another than `ws`.
pub(crate) fn bot(ws: &str, id: String, b: Bot) -> User {
    let org = match b.team {
        Some(t) if !t.is_empty() && t != ws => Org::External(Remote::new(t)),
        _ => Org::Own,
    };
    User {
        id: UserId::new(id),
        workspace: WorkspaceId::new(ws),
        name: Remote::new(b.name.clone()),
        display_name: Remote::new(b.name),
        org,
        bot: true,
        presence: Presence::Unknown,
    }
}

fn message(m: wire::Message, me: Option<&UserId>) -> Option<Message> {
    // A deleted message's leftover: gone. (A deleted thread's own message stays as Slack's
    // `tombstone`, its replies still under it.)
    if m.hidden == Some(true) || m.subtype.as_deref() == Some("message_deleted") {
        return None;
    }
    let ts = Ts(wire::ts(&m.ts)?);
    let thread = match (m.reply_count, m.latest_reply.as_deref().and_then(wire::ts)) {
        (Some(replies), Some(last)) if replies > 0 => Some(ThreadSummary { replies, last_reply: Ts(last) }),
        _ => None,
    };
    let reactions = m
        .reactions
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            let users = r.users.unwrap_or_default();
            let mine = me.is_some_and(|me| users.iter().any(|u| u == me.as_str()));
            let count = r.count.unwrap_or_else(|| u32::try_from(users.len()).unwrap_or(u32::MAX));
            Reaction { name: Remote::new(r.name), count, mine }
        })
        .collect();
    let files: Vec<String> =
        m.files.unwrap_or_default().into_iter().filter_map(|f| f.name.or(f.title)).filter(|n| !n.is_empty()).collect();
    let text =
        crate::blocks::text(m.text.as_deref().unwrap_or_default(), m.blocks.as_ref(), m.attachments.as_ref(), &files);
    Some(Message {
        ts,
        user: UserId::new(m.user.or(m.bot_id).unwrap_or_default()),
        text: Remote::new(text),
        thread,
        reactions,
        edited: m.edited.is_some(),
    })
}
