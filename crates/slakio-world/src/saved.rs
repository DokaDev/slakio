//! The demo's saved items: to-dos in progress (overdue, upcoming, without a reminder), completed
//! ones, and messages kept in Saved, in both workspaces, each a message of the world (one is a
//! thread's own message, one carries hostile text). Due dates are counted from the world's clock
//! ([`now`]), so which are overdue never depends on when the demo runs.
//!
//! [`change`] applies a change the way Slack's `saved.update` / `saved.delete` would, and the
//! demo answers with every item after it.

use crate::World;
use slakio_core::backend::SavedChange;
use slakio_core::model::{SavedId, SavedItem, SavedState, Ts};

const HOUR: u64 = 3_600_000_000;

/// (workspace, conversation, how many messages back from its newest, state, due in hours from
/// [`now`]: negative is overdue).
type Spec = (&'static str, &'static str, usize, SavedState, Option<i64>);

const ITEMS: &[Spec] = &[
    ("TDEMOA", "incidents", 0, SavedState::InProgress, Some(-26)),
    ("TDEMOA", "backend", 1, SavedState::InProgress, Some(-3)),
    ("TDEMOA", "deploys", 0, SavedState::InProgress, Some(20)),
    ("TDEMOA", "frontend", 2, SavedState::InProgress, Some(96)),
    ("TDEMOA", crate::names::HOSTILE, 0, SavedState::InProgress, None),
    ("TDEMOA", "general", 1, SavedState::InProgress, None),
    ("TDEMOA", "design-review", 0, SavedState::Completed, None),
    ("TDEMOA", "announcements", 0, SavedState::Completed, Some(-48)),
    ("TDEMOA", "random", 0, SavedState::Saved, None),
    ("TDEMOA", "alerts", 1, SavedState::Saved, None),
    ("TDEMOA", "Minsu Kim", 0, SavedState::Saved, None),
    ("TDEMOB", "side-project", 0, SavedState::InProgress, Some(-5)),
    ("TDEMOB", "ideas", 0, SavedState::InProgress, Some(30)),
    ("TDEMOB", "general", 0, SavedState::Saved, None),
];

/// The world's clock: an hour after the newest message saved.
pub(crate) fn now(w: &World) -> Ts {
    let newest = ITEMS.iter().filter_map(|s| message(w, s)).map(|m| m.ts).max().unwrap_or_default();
    Ts(newest.0 + HOUR)
}

fn message(w: &World, (ws, name, back, ..): &Spec) -> Option<slakio_core::model::Message> {
    let c = w.find(ws, name)?;
    let n = w.message_count(&c.id);
    w.message(&c.id, n.checked_sub(1 + back)?)
}

/// Every saved item of the demo, in the order they were saved.
pub(crate) fn items(w: &World) -> Vec<SavedItem> {
    let now = now(w);
    let mut out: Vec<SavedItem> = ITEMS
        .iter()
        .enumerate()
        .filter_map(|(i, spec)| {
            let (ws, name, _, state, due) = *spec;
            let c = w.find(ws, name)?;
            let m = message(w, spec)?;
            Some(SavedItem {
                id: SavedId::new(format!("Sv{i:03}")),
                workspace: c.workspace.clone(),
                conversation: c.id.clone(),
                ts: m.ts,
                thread: m.thread.is_some(),
                author: m.user,
                text: m.text,
                // Saved a while after it was written, in the order of the list.
                created: Ts(m.ts.0 + HOUR / 4),
                due: due.map(|h| Ts(now.0.saturating_add_signed(h * HOUR as i64))),
                state,
            })
        })
        .collect();
    out.sort_by_key(|s| s.created);
    out
}

/// Apply `change` to item `id`: `false` when there is no such item or the change does not apply
/// to it (completing a completed to-do, moving a saved message to Saved).
pub(crate) fn change(items: &mut Vec<SavedItem>, id: &SavedId, change: SavedChange) -> bool {
    let Some(at) = items.iter().position(|s| &s.id == id) else { return false };
    let item = &mut items[at];
    match (change, item.state) {
        (SavedChange::Complete, SavedState::InProgress) => item.state = SavedState::Completed,
        (SavedChange::Reopen, SavedState::Completed) => item.state = SavedState::InProgress,
        (SavedChange::MoveToSaved, SavedState::InProgress | SavedState::Completed) => {
            item.state = SavedState::Saved;
            item.due = None;
        }
        (SavedChange::MoveToTodos, SavedState::Saved) => item.state = SavedState::InProgress,
        (SavedChange::Clear, _) => {
            items.remove(at);
        }
        _ => return false,
    }
    true
}
