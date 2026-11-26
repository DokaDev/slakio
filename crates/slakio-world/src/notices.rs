//! A few of Slack's notices about a conversation (someone joined, someone left, the topic was
//! set), after the newest messages of `ideas` in `B side`, so the demo shows them. They come
//! after the channel's own messages and leave those as they were.

use crate::Plan;
use slakio_core::model::{Message, System, Ts};
use slakio_core::sanitize::Remote;

/// The workspace and channel the notices are in.
const CHANNEL: (&str, &str) = ("TDEMOB", "ideas");

/// The topic the last notice sets.
const TOPIC: &str = "One idea per thread, please";

/// The notices, oldest first: what each tells and which of the channel's posters gives it.
fn all() -> [(System, usize); 3] {
    [(System::Joined, 1), (System::Left, 2), (System::Topic(Remote::new(TOPIC)), 0)]
}

/// How many notices the channel `name` of workspace `team` has after its own messages.
pub(crate) fn count(team: &str, name: &str) -> usize {
    if (team, name) == CHANNEL { all().len() } else { 0 }
}

/// Message `index` of the conversation of `plan`, written at `ts`, when it is one of the
/// notices (they are the last messages of the channel).
pub(crate) fn message(plan: &Plan, index: usize, ts: Ts) -> Option<Message> {
    let first = plan.count.checked_sub(count(&plan.team, &plan.name))?;
    let (system, poster) = all().into_iter().nth(index.checked_sub(first)?)?;
    let user = plan.posters.get(poster % plan.posters.len().max(1))?.clone();
    let text = match &system {
        System::Joined => format!("<@{}> has joined the channel", user.as_str()),
        System::Left => format!("<@{}> has left the channel", user.as_str()),
        _ => format!("<@{}> set the channel topic: {TOPIC}", user.as_str()),
    };
    Some(Message {
        ts,
        user,
        text: text.into(),
        thread: None,
        reactions: Vec::new(),
        edited: false,
        files: Vec::new(),
        emoji_pictures: Vec::new(),
        system: Some(Box::new(system)),
    })
}
