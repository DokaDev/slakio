use super::*;
use crate::app::pane::tests::msg;
use slakio_core::model::{ConversationId, WorkspaceId};

fn target(ws: &str) -> Target {
    Target::Conversation { workspace: WorkspaceId::new(ws), conversation: ConversationId::new("C") }
}

fn page(ws: &str, complete: bool) -> Page {
    Page { target: target(ws), messages: Vec::new(), complete, users: Vec::new(), cut: false }
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

#[test]
fn a_thread_read_only_in_part_stays_marked_and_the_cache_s_mark_goes_with_its_messages() {
    let mut tl = Timeline::default();
    tl.add_cached((50..60).map(msg).collect());
    tl.cut = true;
    let fresh = page("T", false);
    tl.add_page(&fresh, (50..60).map(msg).collect());
    assert!(!tl.cut, "the backend's page replaces the cache's, its mark too");
    let cut = Page { cut: true, ..page("T", false) };
    tl.add_page(&cut, (40..50).map(msg).collect());
    tl.add_page(&page("T", true), (30..40).map(msg).collect());
    assert!(tl.cut, "an older page does not unsay it");
}

fn ts(tl: &Timeline) -> Vec<u64> {
    tl.items.iter().map(|m| m.ts.0).collect()
}

#[test]
fn a_live_message_goes_in_its_place_among_what_is_loaded_only() {
    let mut tl = Timeline::default();
    assert!(!tl.put_live(msg(70), false), "nothing loaded: the page brings it");
    tl.add_page(&page("T", false), (50..60).map(msg).collect());
    assert!(tl.put_live(msg(70), false), "newest");
    assert!(tl.put_live(msg(55), false), "a copy at its ts replaces it");
    assert!(!tl.put_live(msg(40), false), "older than what is loaded: not read there");
    assert!(tl.put_live(msg(65), false), "late, but among what is loaded");
    assert_eq!(ts(&tl), [50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 65, 70]);
    assert!(!tl.put_live(msg(80), true), "an edit of a message not loaded adds nothing");
    let mut edited = msg(70);
    edited.edited = true;
    tl.items[11].thread = Some(ThreadSummary { replies: 2, last_reply: Ts(90) });
    assert!(tl.put_live(edited, true));
    assert!(tl.items[11].edited);
    assert_eq!(tl.items[11].thread.map(|t| t.replies), Some(2), "a copy without its replies keeps the count");
    // Once the start is loaded, an old one has its place too.
    let mut whole = Timeline::default();
    whole.add_page(&page("T", true), (50..52).map(msg).collect());
    assert!(whole.put_live(msg(10), false));
    assert_eq!(ts(&whole), [10, 50, 51]);
    assert!(whole.remove(Ts(50)) && !whole.remove(Ts(50)));
}

#[test]
fn a_reply_counts_once_and_a_deleted_one_comes_off() {
    let mut tl = Timeline::default();
    tl.add_page(&page("T", true), (1..3).map(msg).collect());
    assert!(tl.add_reply(Ts(1), Ts(10)));
    assert!(!tl.add_reply(Ts(1), Ts(10)), "the same reply again");
    assert!(!tl.add_reply(Ts(1), Ts(5)), "not newer than the last counted");
    assert!(tl.add_reply(Ts(1), Ts(11)));
    assert_eq!(tl.items[0].thread, Some(ThreadSummary { replies: 2, last_reply: Ts(11) }));
    assert!(!tl.add_reply(Ts(99), Ts(12)), "a parent not loaded");
    assert!(tl.drop_reply(Ts(1)) && tl.drop_reply(Ts(1)));
    assert_eq!(tl.items[0].thread, None);
    assert!(!tl.drop_reply(Ts(1)));
}

#[test]
fn catching_up_is_the_truth_for_its_span_and_never_hides_a_gap() {
    let joins = |items: std::ops::Range<u64>, page_ts: Vec<u64>, complete: bool| {
        let mut tl = Timeline::default();
        tl.add_page(&page("T", false), items.map(msg).collect());
        tl.catchup = Some(Generation(9));
        tl.catch_up(&page("T", complete), page_ts.into_iter().map(msg).collect());
        assert_eq!(tl.catchup, None);
        tl
    };
    // Overlapping: 57 was deleted meanwhile, 60 and 61 are new.
    let tl = joins(50..60, vec![55, 56, 58, 59, 60, 61], false);
    assert_eq!(ts(&tl), [50, 51, 52, 53, 54, 55, 56, 58, 59, 60, 61]);
    // A gap between the newest loaded and the page: the page alone.
    let tl = joins(50..60, vec![70, 71], false);
    assert_eq!(ts(&tl), [70, 71]);
    assert!(!tl.complete && !tl.paged);
    // The whole conversation in one page.
    let tl = joins(50..60, vec![52, 70], true);
    assert_eq!(ts(&tl), [52, 70]);
    assert!(tl.complete);
    // Nothing left at all.
    let tl = joins(50..60, Vec::new(), true);
    assert!(tl.items.is_empty() && tl.complete);
    // An empty page that says nothing of the start changes nothing.
    let tl = joins(50..60, Vec::new(), false);
    assert_eq!(tl.items.len(), 10);
}

#[test]
fn a_failed_catch_up_leaves_what_was_loaded() {
    let mut store = TimelineStore::default();
    let tl = store.entry(&target("T"));
    tl.add_page(&page("T", false), (50..60).map(msg).collect());
    tl.catchup = Some(Generation(4));
    assert_eq!(store.fail(Generation(4), FailKind::Network, Instant::now()), Some(target("T")));
    let tl = store.get(&target("T")).unwrap();
    assert!(tl.catchup.is_none() && tl.failed.is_none() && tl.items.len() == 10);
}
