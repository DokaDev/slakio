use super::*;
use slakio_core::backend::{Generation, Page};
use slakio_core::model::{ConversationId, WorkspaceId};

fn target() -> Target {
    Target::Conversation { workspace: WorkspaceId::new("T"), conversation: ConversationId::new("C") }
}

pub(crate) fn msg(ts: u64) -> Shown {
    Shown {
        ts: Ts(ts),
        user: UserId::new("U"),
        author: Safe::default(),
        own: false,
        text: Safe::default(),
        body: Vec::new(),
        thread: None,
        reactions: Vec::new(),
        edited: false,
    }
}

/// Messages `from..to` arrive in `tl`.
fn arrive(tl: &mut Timeline, from: u64, to: u64, complete: bool) {
    let page = Page { target: target(), messages: Vec::new(), complete, users: Vec::new(), cut: false };
    tl.add_page(&page, (from..to).map(msg).collect());
}

#[test]
fn the_newest_page_first_then_one_older_page_per_gesture_near_the_top() {
    let mut p = Pane::new(target());
    let mut tl = Timeline::default();
    assert_eq!(p.wants(&tl), Some(None), "the newest page");
    tl.pending = Some(Generation(1));
    assert_eq!(p.wants(&tl), None, "one request at a time");
    arrive(&mut tl, 100, 300, false);
    assert_eq!(p.wants(&tl), None, "nothing selected near the top");
    p.step(-1, &tl);
    assert_eq!(p.selected_index(&tl), Some(199), "the first move selects the newest");
    p.select_index(PREFETCH - 1, &tl);
    p.older = false;
    assert_eq!(p.wants(&tl), None, "a selection near the top asks nothing by itself");
    p.step(-1, &tl);
    assert_eq!(p.wants(&tl), Some(Some(Ts(100))), "moving up near the top does");
    p.older = false;
    arrive(&mut tl, 0, 100, false);
    assert_eq!(p.selected_index(&tl), Some(PREFETCH - 2 + 100), "the selection stays on its message");
    assert_eq!(p.wants(&tl), None, "a page that arrives asks for no other");
    p.step(1, &tl);
    assert_eq!(p.wants(&tl), None, "moving down asks for none");
}

#[test]
fn gg_selects_the_oldest_loaded_and_wants_one_more_page() {
    let mut p = Pane::new(target());
    let mut tl = Timeline::default();
    arrive(&mut tl, 400, 600, false);
    p.select_oldest(&tl);
    assert_eq!(p.selected_index(&tl), Some(0));
    assert_eq!(p.wants(&tl), Some(Some(Ts(400))));
    tl.queued = true;
    assert_eq!(p.wants(&tl), None, "one is queued already");
    tl.queued = false;
    // A gesture asks again for an older page that failed, whatever Slack said, but not one the
    // sign-in stopped, nor one waiting out Slack's pause.
    use slakio_core::backend::FailKind;
    for kind in [FailKind::Network, FailKind::Api { code: "internal_error".into() }, FailKind::CapReached] {
        tl.failed = Some(kind);
        assert_eq!(p.wants(&tl), Some(Some(Ts(400))), "{:?}", tl.failed);
    }
    tl.failed = Some(FailKind::Auth);
    assert_eq!(p.wants(&tl), None, "signed out: asking again cannot help");
    tl.failed = Some(FailKind::RateLimited { retry_after: None });
    tl.retry_at = Some(std::time::Instant::now());
    assert_eq!(p.wants(&tl), None, "asked again by itself after Slack's pause");
    (tl.failed, tl.retry_at) = (None, None);
    p.older = false;
    arrive(&mut tl, 200, 400, false);
    assert_eq!(p.selected_index(&tl), Some(200), "the page lands above, the selection stays");
    assert_eq!(p.wants(&tl), None);
    arrive(&mut tl, 0, 200, true);
    p.select_oldest(&tl);
    assert_eq!(p.wants(&tl), None, "complete: nothing older");
}

#[test]
fn steps_stop_at_the_ends_and_the_range_is_ordered() {
    let mut p = Pane::new(target());
    let mut tl = Timeline::default();
    p.step(1, &tl);
    assert_eq!(p.selected, None, "nothing loaded, nothing selected");
    arrive(&mut tl, 0, 5, true);
    p.step(1, &tl);
    p.step(1, &tl);
    assert_eq!(p.selected_index(&tl), Some(4));
    p.visual = Some(Ts(4));
    p.step(-1, &tl);
    p.step(-1, &tl);
    assert_eq!(p.range(&tl), Some((2, 4)));
    for _ in 0..10 {
        p.step(-1, &tl);
    }
    assert_eq!(p.range(&tl), Some((0, 4)));
}

#[test]
fn an_echo_goes_after_the_newest_and_brings_the_view_back_to_it() {
    let mut p = Pane::new(target());
    let mut tl = Timeline::default();
    arrive(&mut tl, 0, 3, true);
    p.select_index(0, &tl);
    p.bottom.set(Some(Ts(1)));
    echo(&mut tl.items, UserId::new("ME"), Safe::default(), slakio_core::sanitize::sanitize_block("hi"));
    p.echoed();
    let last = tl.items.last().unwrap();
    assert!(last.own && last.ts > Ts(2));
    assert_eq!((p.selected, p.bottom.get()), (None, None));
}

#[test]
fn the_selection_and_the_anchor_stay_on_their_messages_whatever_arrives() {
    let mut p = Pane::new(target());
    let mut tl = Timeline::default();
    arrive(&mut tl, 100, 200, false);
    p.select_index(10, &tl);
    p.visual = Some(Ts(120));
    p.bottom.set(Some(Ts(150)));
    arrive(&mut tl, 0, 100, true);
    echo(&mut tl.items, UserId::new("ME"), Safe::default(), Safe::default());
    assert_eq!(p.selected_index(&tl).map(|i| tl.items[i].ts), Some(Ts(110)), "older messages came before it");
    assert_eq!(p.range(&tl), Some((110, 120)));
    assert_eq!(p.bottom.get(), Some(Ts(150)));
    // A message that is gone: the selection moves to the next newer one.
    tl.items.retain(|m| m.ts != Ts(110));
    assert_eq!(p.selected_index(&tl).map(|i| tl.items[i].ts), Some(Ts(111)));
}

#[test]
fn a_deleted_selection_moves_to_the_nearest_newer_message_else_the_older_one() {
    let mut p = Pane::new(target());
    let mut tl = Timeline::default();
    arrive(&mut tl, 0, 5, true);
    p.select_index(2, &tl);
    tl.items.retain(|m| m.ts != Ts(2));
    assert_eq!(p.selected_index(&tl).map(|i| tl.items[i].ts), Some(Ts(3)), "the next newer");
    p.selected = Some(Ts(4));
    tl.items.retain(|m| m.ts != Ts(4));
    assert_eq!(p.selected_index(&tl).map(|i| tl.items[i].ts), Some(Ts(3)), "nothing newer: the older one");
    tl.items.clear();
    assert_eq!(p.selected_index(&tl), None, "nothing left");
}
