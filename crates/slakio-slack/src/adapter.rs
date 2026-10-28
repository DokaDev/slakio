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
use slakio_core::model::{Message, Org, Presence, Reaction, Target, ThreadSummary, Ts, User, UserId, WorkspaceId};
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

/// Run the backend of `client` on a task of its own (inside a Tokio runtime).
pub fn serve(client: Client) -> BackendHandle {
    let (handle, end) = BackendHandle::pair(CAPABILITIES, BOUND);
    tokio::spawn(run(Arc::new(client), end));
    handle
}

/// What the backend learns in a session: the user's own id once booted (whose reactions are
/// "mine"), the people named so far, the threads read.
#[derive(Default)]
struct Session {
    me: Me,
    people: People,
    threads: Threads,
}

pub use crate::people::LOOKUP_BATCH;

async fn run(client: Arc<Client>, mut end: BackendEnd) {
    let session: Arc<Session> = Arc::default();
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
        match command {
            Command::Boot => {
                tasks.spawn(async move { answer(&tx, id, boot(&client, &ctx.me, &ctx.people).await).await });
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
    let limit = limit.clamp(1, PAGE_MAX);
    let channel = target.conversation().as_str().to_string();
    let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (messages, complete, bots) = match &target {
        Target::Thread { thread, .. } => {
            ctx.threads.page(client, me.as_ref(), &channel, *thread, before, limit).await?
        }
        Target::Conversation { .. } => {
            let count = limit.to_string();
            let history: wire::History = match before.map(|ts| ts.to_string()) {
                None => {
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
                    view.history.unwrap_or_default()
                }
                Some(latest) => {
                    let params = [
                        ("channel", channel.as_str()),
                        ("latest", latest.as_str()),
                        ("limit", count.as_str()),
                        ("ignore_replies", "true"),
                    ];
                    client.call(&Call::form(Method::ConversationsHistory, &params), Priority::User).await?
                }
            };
            // Unknown is not complete: only Slack's own "no more" says the start is in this page;
            // but an older page with nothing in it has nothing older to give either (asking
            // again would ask for the same).
            let has_more = history.has_more;
            let (mut messages, bots) = read(history.messages.unwrap_or_default(), me.as_ref());
            messages.retain(|m| before.is_none_or(|b| m.ts < b));
            let complete = has_more == Some(false) || (before.is_some() && messages.is_empty());
            (messages, complete, bots)
        }
    };
    // The authors by name: what the view brought, else looked up (in batches); not the user,
    // nor a bot (named by the name it posts as).
    let authors: Vec<String> = messages
        .iter()
        .map(|m| m.user.as_str())
        .filter(|u| !u.is_empty() && me.as_ref().is_none_or(|me| me.as_str() != *u) && !bots.contains_key(*u))
        .map(str::to_string)
        .collect();
    let mut users = ctx.people.names(client, &authors, Priority::User).await;
    if let Some(ws) = ctx.people.team() {
        users.extend(bots.into_iter().map(|(id, name)| bot(&ws, id, name)));
    }
    Ok(Page { target, messages, complete, users })
}

/// The bots that posted in a page, by id: the name each posts as.
pub(crate) type Bots = HashMap<String, String>;

/// `raw` as messages, oldest first, each once (a message Slack lists twice is shown once), the
/// deleted ones left out; and the bots among their authors.
pub(crate) fn read(raw: Vec<wire::Message>, me: Option<&UserId>) -> (Vec<Message>, Bots) {
    let mut bots = Bots::new();
    let mut messages: Vec<Message> = raw
        .into_iter()
        .filter_map(|m| {
            if let (None, Some(id), Some(name)) = (&m.user, &m.bot_id, &m.username) {
                bots.insert(id.clone(), name.clone());
            }
            message(m, me)
        })
        .collect();
    messages.sort_by_key(|m| m.ts);
    messages.dedup_by_key(|m| m.ts);
    (messages, bots)
}

/// A bot of workspace `ws` as a person: named as it posts.
fn bot(ws: &str, id: String, name: String) -> User {
    User {
        id: UserId::new(id),
        workspace: WorkspaceId::new(ws),
        name: Remote::new(name.clone()),
        display_name: Remote::new(name),
        org: Org::Own,
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
