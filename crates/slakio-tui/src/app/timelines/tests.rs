use super::*;
use crate::app::pane::tests::msg;
use slakio_core::model::{ConversationId, WorkspaceId};

fn target(ws: &str) -> Target {
    Target::Conversation { workspace: WorkspaceId::new(ws), conversation: ConversationId::new("C") }
}

fn page(ws: &str, complete: bool) -> Page {
    Page { target: target(ws), messages: Vec::new(), complete }
}

#[test]
fn a_late_or_overlapping_page_never_doubles_messages() {
    let mut tl = Timeline::default();
    assert_eq!(tl.add_page(&page("T", false), (50..60).map(msg).collect()), 10);
    assert_eq!(tl.add_page(&page("T", false), (40..55).map(msg).collect()), 10, "only what is older");
    let ts: Vec<u64> = tl.items.iter().map(|m| m.ts.0).collect();
    assert_eq!(ts, (40..60).collect::<Vec<_>>());
    assert_eq!(tl.position(Ts(45)), Some(5));
}

#[test]
fn a_timeline_is_kept_per_workspace_and_answers_only_its_request() {
    let mut store = TimelineStore::default();
    store.entry(&target("T1")).pending = Some(Generation(1));
    store.entry(&target("T2")).pending = Some(Generation(2));
    assert!(store.awaiting(&target("T1"), Generation(2)).is_none(), "another workspace's request");
    assert!(store.awaiting(&target("T2"), Generation(2)).is_some());
    store.retain(|t| *t == target("T2"));
    assert!(store.get(&target("T1")).is_none() && store.get(&target("T2")).is_some());
}
