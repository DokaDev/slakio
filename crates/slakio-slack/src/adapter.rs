//! The Slack backend behind a [`BackendHandle`]: a task that owns the [`BackendEnd`], reads the
//! UI's requests and answers each one exactly once (its event, or `Failed`), never with
//! silence. A change is refused before anything is sent (the backend is read-only). A newer
//! `History` for the same target supersedes one still in flight: the older one is dropped
//! unanswered, since the UI no longer waits for it. When the UI is gone the task ends, and
//! what it had in flight with it.

use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use crate::wire;
use slakio_core::backend::{
    BackendEnd, BackendHandle, Capabilities, Command, Envelope, Event, Generation, Page, Request, Snapshot,
    refuse_change,
};
use slakio_core::model::{
    Conversation, ConversationId, ConversationKind, Message, Org, Presence, Reaction, Section, SectionId, SectionKind,
    Target, ThreadSummary, Ts, User, UserId, Workspace, WorkspaceColor, WorkspaceId,
};
use slakio_core::sanitize::Remote;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinSet};

/// How many requests, and how many events, wait in the handle's channels at most.
pub const BOUND: usize = 64;

/// The most messages one page asks for: what the web client shows first.
pub const PAGE_MAX: u32 = 28;

/// What the backend can do: read, never change.
pub const CAPABILITIES: Capabilities = Capabilities { demo: false, read_only: true };

/// Run the backend of `client` on a task of its own (inside a Tokio runtime).
pub fn serve(client: Client) -> BackendHandle {
    let (handle, end) = BackendHandle::pair(CAPABILITIES, BOUND);
    tokio::spawn(run(Arc::new(client), end));
    handle
}

/// The user's own id once the workspace has booted (whose reactions are "mine").
type Me = Arc<Mutex<Option<UserId>>>;

async fn run(client: Arc<Client>, mut end: BackendEnd) {
    let me: Me = Arc::default();
    let mut tasks = JoinSet::new();
    let mut in_flight: HashMap<Target, AbortHandle> = HashMap::new();
    while let Some(request) = end.rx.recv().await {
        while tasks.try_join_next().is_some() {}
        if let Some(refused) = refuse_change(&request) {
            if end.tx.send(refused).await.is_err() {
                return;
            }
            continue;
        }
        let (client, tx, me) = (Arc::clone(&client), end.tx.clone(), Arc::clone(&me));
        let Request { id, command, .. } = request;
        match command {
            Command::Boot => {
                tasks.spawn(async move { answer(&tx, id, boot(&client, &me).await).await });
            }
            Command::History { target, before, limit } => {
                if let Some(older) = in_flight.remove(&target) {
                    older.abort();
                }
                let key = target.clone();
                let task = tasks.spawn(async move {
                    let page = history(&client, &me, target, before, limit).await;
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

/// The workspace from `client.userBoot` (it cannot do without), its unread marks from
/// `client.counts` (it can).
async fn boot(client: &Client, me: &Me) -> Result<Event, SlackError> {
    let call = Call::form(
        Method::ClientUserBoot,
        &[
            ("version_all_channels", "false"),
            ("return_all_relevant_mpdms", "true"),
            ("omit_extras", "feature_usage_data,plan_info,salesforce_features"),
        ],
    );
    let boot: wire::UserBoot = client.call(&call, Priority::User).await?;
    let (Some(team), Some(self_)) = (boot.team, boot.me) else {
        return Err(SlackError::Network { detail: "the start answer has no team or user".to_string() });
    };
    client.set_team(&team.id);
    *me.lock().unwrap_or_else(PoisonError::into_inner) = Some(UserId::new(&self_.id));
    let counts: Option<wire::Counts> =
        client.call(&Call::form(Method::ClientCounts, &[]), Priority::Background).await.ok();
    Ok(Event::Booted(Box::new(snapshot(&team, &self_, boot.channels, boot.ims, counts))))
}

fn snapshot(
    team: &wire::Team,
    me: &wire::Me,
    channels: Option<Vec<wire::Channel>>,
    ims: Option<Vec<wire::Im>>,
    counts: Option<wire::Counts>,
) -> Snapshot {
    let ws = WorkspaceId::new(&team.id);
    let mut unread: HashMap<String, (u32, u32)> = HashMap::new();
    if let Some(c) = counts {
        for n in [c.channels, c.mpims, c.ims].into_iter().flatten().flatten() {
            let mentions = n.mention_count.unwrap_or(0);
            let unreads = if n.has_unreads == Some(true) { mentions.max(1) } else { 0 };
            unread.insert(n.id, (unreads, mentions.min(unreads)));
        }
    }
    let section = |id: &str, name: &str, kind| Section {
        id: SectionId::new(format!("{}:{id}", team.id)),
        workspace: ws.clone(),
        name: Remote::new(name),
        kind,
    };
    let sections = vec![
        section("channels", "Channels", SectionKind::Channels),
        section("dms", "Direct messages", SectionKind::DirectMessages),
    ];
    let conversation = |id: String, name: String, kind: ConversationKind, external: bool, section: &Section| {
        let (unread, mentions) = unread.get(&id).copied().unwrap_or_default();
        Conversation {
            id: ConversationId::new(id),
            workspace: ws.clone(),
            kind,
            name: Remote::new(name),
            section: section.id.clone(),
            external,
            muted: false,
            unread,
            mentions,
        }
    };
    let mut conversations: Vec<Conversation> = channels
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.is_archived != Some(true) && c.is_mpim != Some(true))
        .map(|c| {
            let name = c.name.unwrap_or_else(|| c.id.clone());
            let kind = ConversationKind::Channel { private: c.is_private == Some(true) };
            conversation(c.id, name, kind, c.is_ext_shared == Some(true), &sections[0])
        })
        .collect();
    conversations.extend(ims.unwrap_or_default().into_iter().filter_map(|im| {
        let user = im.user?;
        Some(conversation(im.id, user.clone(), ConversationKind::Dm { user: UserId::new(user) }, false, &sections[1]))
    }));
    let me_name = me.name.clone().unwrap_or_else(|| me.id.clone());
    Snapshot {
        workspaces: vec![Workspace {
            id: ws.clone(),
            name: Remote::new(team.name.clone().unwrap_or_else(|| team.id.clone())),
            color: WorkspaceColor(0),
            me: UserId::new(&me.id),
        }],
        users: vec![User {
            id: UserId::new(&me.id),
            workspace: ws,
            name: Remote::new(me_name.clone()),
            display_name: Remote::new(me_name),
            org: Org::Own,
            bot: false,
            presence: Presence::Unknown,
        }],
        sections,
        conversations,
        // Saved items are not read in this version: unknown, not none.
        saved: None,
        taken: Ts(now_micros()),
    }
}

/// A page of `target`: the newest through `conversations.view`, older ones through
/// `conversations.history`, a thread through `conversations.replies`.
async fn history(client: &Client, me: &Me, target: Target, before: Option<Ts>, limit: u32) -> Result<Page, SlackError> {
    let limit = limit.clamp(1, PAGE_MAX).to_string();
    let channel = target.conversation().as_str().to_string();
    let latest = before.map(|ts| ts.to_string());
    let history: wire::History = match (&target, &latest) {
        (Target::Thread { thread, .. }, _) => {
            let ts = thread.to_string();
            let mut params = vec![("channel", channel.as_str()), ("ts", ts.as_str()), ("limit", limit.as_str())];
            if let Some(l) = &latest {
                params.push(("latest", l));
            }
            client.call(&Call::form(Method::ConversationsReplies, &params), Priority::User).await?
        }
        (Target::Conversation { .. }, None) => {
            let call = Call::form(
                Method::ConversationsView,
                &[
                    ("channel", channel.as_str()),
                    ("count", limit.as_str()),
                    ("ignore_replies", "true"),
                    ("no_members", "true"),
                ],
            );
            let view: wire::View = client.call(&call, Priority::User).await?;
            view.history.unwrap_or_default()
        }
        (Target::Conversation { .. }, Some(l)) => {
            let params = [
                ("channel", channel.as_str()),
                ("latest", l.as_str()),
                ("limit", limit.as_str()),
                ("ignore_replies", "true"),
            ];
            client.call(&Call::form(Method::ConversationsHistory, &params), Priority::User).await?
        }
    };
    let me = me.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let mut messages: Vec<Message> =
        history.messages.unwrap_or_default().into_iter().filter_map(|m| message(m, me.as_ref())).collect();
    messages.sort_by_key(|m| m.ts);
    // Unknown is not complete: only Slack's own "no more" says the start is in this page.
    let complete = history.has_more == Some(false);
    Ok(Page { target, messages, complete })
}

fn message(m: wire::Message, me: Option<&UserId>) -> Option<Message> {
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
    Some(Message {
        ts,
        user: UserId::new(m.user.unwrap_or_default()),
        text: Remote::new(m.text.unwrap_or_default()),
        thread,
        reactions,
        edited: m.edited.is_some(),
    })
}

fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}
