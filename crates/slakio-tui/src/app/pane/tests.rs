use super::*;
use slakio_core::model::{ConversationId, WorkspaceId};

fn target() -> Target {
    Target::Conversation { workspace: WorkspaceId::new("T"), conversation: ConversationId::new("C") }
}

fn msg(ts: u64) -> Shown {
    Shown {
        ts: Ts(ts),
        user: UserId::new("U"),
        author: Safe::default(),
        own: false,
        text: Safe::default(),
        thread: None,
        reactions: Vec::new(),
        edited: false,
    }
}

fn page(from: u64, to: u64, complete: bool) -> (Page, Vec<Shown>) {
    let shown: Vec<Shown> = (from..to).map(msg).collect();
    (Page { target: target(), messages: Vec::new(), complete }, shown)
}

#[test]
fn the_newest_page_first_then_older_pages_as_the_selection_nears_the_top() {
    let mut p = Pane::new(target());
    assert_eq!(p.wants(), Some(None), "the newest page");
    p.pending = Some(Generation(1));
    assert_eq!(p.wants(), None, "one request at a time");
    let (pg, shown) = page(100, 300, false);
    p.add_page(&pg, shown);
    assert_eq!(p.wants(), None, "nothing selected near the top");
    p.step(-1);
    assert_eq!(p.selected, Some(199), "the first move selects the newest");
    p.selected = Some(PREFETCH - 1);
    assert_eq!(p.wants(), Some(Some(Ts(100))));
    let (pg, shown) = page(0, 100, true);
    p.add_page(&pg, shown);
    assert_eq!(p.items.len(), 300);
    assert_eq!(p.selected, Some(PREFETCH - 1 + 100), "the selection stays on its message");
    assert_eq!(p.wants(), None, "complete");
}

#[test]
fn a_late_or_overlapping_page_never_doubles_messages() {
    let mut p = Pane::new(target());
    let (pg, shown) = page(50, 60, false);
    p.add_page(&pg, shown);
    let (pg, shown) = page(40, 55, false);
    p.add_page(&pg, shown);
    let ts: Vec<u64> = p.items.iter().map(|m| m.ts.0).collect();
    assert_eq!(ts, (40..60).collect::<Vec<_>>());
}

#[test]
fn gg_keeps_loading_until_the_oldest_message_is_selected() {
    let mut p = Pane::new(target());
    let (pg, shown) = page(400, 600, false);
    p.add_page(&pg, shown);
    p.select_oldest();
    assert_eq!((p.selected, p.to_oldest), (Some(0), true));
    assert_eq!(p.wants(), Some(Some(Ts(400))));
    let (pg, shown) = page(200, 400, false);
    p.add_page(&pg, shown);
    assert_eq!(p.selected, Some(0), "still the oldest loaded");
    let (pg, shown) = page(0, 200, true);
    p.add_page(&pg, shown);
    assert_eq!((p.selected, p.to_oldest, p.items[0].ts), (Some(0), false, Ts(0)));
    p.step(1);
    assert_eq!(p.selected, Some(1));
}

#[test]
fn steps_stop_at_the_ends_and_the_range_is_ordered() {
    let mut p = Pane::new(target());
    p.step(1);
    assert_eq!(p.selected, None, "nothing loaded, nothing selected");
    let (pg, shown) = page(0, 5, true);
    p.add_page(&pg, shown);
    p.step(1);
    p.step(1);
    assert_eq!(p.selected, Some(4));
    p.visual = Some(4);
    p.step(-1);
    p.step(-1);
    assert_eq!(p.range(), Some((2, 4)));
    for _ in 0..10 {
        p.step(-1);
    }
    assert_eq!(p.range(), Some((0, 4)));
}

#[test]
fn an_echo_goes_after_the_newest_and_brings_the_view_back_to_it() {
    let mut p = Pane::new(target());
    let (pg, shown) = page(0, 3, true);
    p.add_page(&pg, shown);
    p.selected = Some(0);
    p.bottom.set(Some(1));
    p.echo(UserId::new("ME"), Safe::default(), slakio_core::sanitize::sanitize_block("hi"));
    let last = p.items.last().unwrap();
    assert!(last.own && last.ts > Ts(2));
    assert_eq!((p.selected, p.bottom.get()), (None, None));
}
