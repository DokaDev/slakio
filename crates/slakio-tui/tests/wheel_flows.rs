//! The mouse wheel moves the view, not the selection: in a conversation, a thread and the list
//! panel the selected row stays where it is, on screen or not, and the next key that moves it
//! brings it back into view. Up at the top of what is loaded it asks for one older page, as a
//! key does; live messages do not move a view the wheel moved; a click selects the row under the
//! pointer in the view as it is; in split panes the wheel moves the pane under it.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use demo::Demo;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, Generation, Live};
use slakio_core::model::{Message, Ts};
use slakio_core::sanitize::Remote;
use slakio_tui::app::{PaneHandle, PaneKind};
use std::time::Duration;

/// The pane's messages area.
fn area(d: &Demo, kind: PaneKind) -> Rect {
    d.app.message_area(d.pane(kind).unwrap().handle()).unwrap()
}

/// The wheel `n` notches over `kind`'s messages (up when `up`), the frame drawn after each.
fn wheel(d: &mut Demo, kind: PaneKind, up: bool, n: usize) {
    let a = area(d, kind);
    let ev = if up { MouseEventKind::ScrollUp } else { MouseEventKind::ScrollDown };
    for _ in 0..n {
        d.mouse(ev, a.x + 5, a.y + a.height / 2);
        d.screen();
    }
}

/// The selected message's timestamp.
fn chosen(d: &Demo, kind: PaneKind) -> Option<Ts> {
    let p = d.pane(kind).unwrap();
    p.selected().map(|i| p.messages()[i].ts)
}

/// The rows drawn last, as (message timestamp, screen row).
fn view(d: &Demo, kind: PaneKind) -> Vec<(Ts, u16)> {
    let p = d.pane(kind).unwrap();
    p.drawn_rows().into_iter().map(|(i, y)| (p.messages()[i].ts, y)).collect()
}

fn shows(d: &Demo, kind: PaneKind, ts: Ts) -> bool {
    view(d, kind).iter().any(|(t, _)| *t == ts)
}

#[test]
fn the_wheel_moves_a_conversation_view_and_a_key_brings_the_selection_back() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("k k");
    d.screen();
    let sel = chosen(&d, PaneKind::Conversation).expect("a selection");
    let before = view(&d, PaneKind::Conversation);
    wheel(&mut d, PaneKind::Conversation, true, 1);
    assert_eq!(chosen(&d, PaneKind::Conversation), Some(sel), "the selection stays");
    let after = view(&d, PaneKind::Conversation);
    assert_ne!(after, before, "the view moved");
    // Three rows a notch: what was on row y is on row y + 3.
    let (ts, y) = before[0];
    assert!(after.contains(&(ts, y + 3)), "{before:?} -> {after:?}");
    wheel(&mut d, PaneKind::Conversation, true, 20);
    assert_eq!(chosen(&d, PaneKind::Conversation), Some(sel));
    assert!(!shows(&d, PaneKind::Conversation, sel), "the selection went off screen");
    // A key moves the selection and brings it back into view.
    d.keys("k");
    d.screen();
    let now = chosen(&d, PaneKind::Conversation).unwrap();
    assert!(now < sel && shows(&d, PaneKind::Conversation, now));
    // Down to the end: the view follows the newest again.
    wheel(&mut d, PaneKind::Conversation, false, 200);
    let newest = d.pane(PaneKind::Conversation).unwrap().messages().last().unwrap().ts;
    assert!(shows(&d, PaneKind::Conversation, newest));
}

#[test]
fn the_wheel_moves_a_thread_view_not_its_selection() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    d.keys("k");
    d.screen();
    let sel = chosen(&d, PaneKind::Thread).expect("a reply selected");
    let before = view(&d, PaneKind::Thread);
    wheel(&mut d, PaneKind::Thread, true, 30);
    assert_eq!(chosen(&d, PaneKind::Thread), Some(sel));
    assert_ne!(view(&d, PaneKind::Thread), before);
    assert!(!shows(&d, PaneKind::Thread, sel));
    d.keys("j");
    d.screen();
    let now = chosen(&d, PaneKind::Thread).unwrap();
    assert!(shows(&d, PaneKind::Thread, now), "back in view");
}

#[test]
fn the_wheel_moves_the_list_not_its_cursor_and_a_key_brings_it_back() {
    let mut d = Demo::new(120, 40);
    let list = d.app.areas().list.unwrap();
    let cursor = d.app.list_cursor();
    for _ in 0..5 {
        d.mouse(MouseEventKind::ScrollDown, list.x + 3, 5);
    }
    assert_eq!(d.app.list_top(), 15);
    assert_eq!(d.app.list_cursor(), cursor, "the cursor stays on its row, off screen");
    d.keys("j");
    let rows_at = d.app.list_parts().unwrap().rows;
    let (top, at) = (d.app.list_top(), d.app.list_cursor());
    assert!(at > cursor && at >= top && at < top + usize::from(rows_at.height), "{top} {at}");
    // A click lands on the row under the pointer in the view as it is.
    for _ in 0..3 {
        d.mouse(MouseEventKind::ScrollDown, list.x + 3, 5);
    }
    let top = d.app.list_top();
    let rows = d.app.list_rows();
    let row = (top..top + 10).find(|&r| matches!(rows[r], slakio_tui::app::model::Row::Conversation(_))).unwrap();
    let y = rows_at.y + (row - top) as u16;
    d.mouse(MouseEventKind::Down(MouseButton::Left), list.x + 3, y);
    assert_eq!(d.app.list_cursor(), row);
}

#[test]
fn a_click_after_the_wheel_selects_the_message_under_the_pointer() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("j");
    let sel = chosen(&d, PaneKind::Conversation);
    wheel(&mut d, PaneKind::Conversation, true, 10);
    assert_eq!(chosen(&d, PaneKind::Conversation), sel, "the wheel left the selection");
    let (ts, y) = view(&d, PaneKind::Conversation)[2];
    let a = area(&d, PaneKind::Conversation);
    d.mouse(MouseEventKind::Down(MouseButton::Left), a.x + 5, y);
    assert_eq!(chosen(&d, PaneKind::Conversation), Some(ts));
    d.screen();
    assert!(view(&d, PaneKind::Conversation).contains(&(ts, y)), "the view stays where it was");
}

#[test]
fn in_split_panes_the_wheel_moves_the_pane_under_it() {
    let mut d = Demo::new(160, 40);
    d.open("long-threads");
    d.keys("space w v");
    d.screen();
    let panes: Vec<(PaneHandle, Rect)> = d.app.panes_on_screen().into_iter().map(|(p, r)| (p.handle(), r)).collect();
    assert_eq!(panes.len(), 2);
    let rows = |d: &Demo, h: PaneHandle| d.app.pane_for(h).unwrap().drawn_rows();
    let (left, right) = (panes[0].0, panes[1].0);
    let before = (rows(&d, left), rows(&d, right));
    let a = d.app.message_area(left).unwrap();
    let sel = d.app.pane_for(left).unwrap().selected();
    d.mouse(MouseEventKind::ScrollUp, a.x + 5, a.y + 3);
    d.screen();
    assert_ne!(rows(&d, left), before.0, "the pane under the wheel moved");
    assert_eq!(rows(&d, right), before.1, "the other one did not");
    assert_eq!(d.app.pane_for(left).unwrap().selected(), sel);
}

/// The held requests for an older page.
fn older(s: &Scripted) -> usize {
    let asked = |c: &Command| matches!(c, Command::History { before: Some(_), .. });
    s.b.held.iter().filter(|r| asked(&r.command)).count() + s.b.log.iter().filter(|r| asked(&r.command)).count()
}

/// Booted, `#big-history` open with its newest page.
fn big() -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.list_cursor_on("big-history");
    s.keys("enter");
    let i = s.b.held.len() - 1;
    s.b.answer(i);
    s.pump();
    s.d.screen();
    s
}

#[test]
fn up_to_the_top_the_wheel_asks_for_one_older_page() {
    let mut s = big();
    s.keys("k");
    let sel = chosen(&s.d, PaneKind::Conversation);
    let loaded = s.d.pane(PaneKind::Conversation).unwrap().messages().len();
    for _ in 0..400 {
        wheel(&mut s.d, PaneKind::Conversation, true, 1);
        s.d.now += Duration::from_millis(100);
        s.pump();
    }
    assert_eq!(older(&s), 1, "one older page, however many notches");
    assert_eq!(chosen(&s.d, PaneKind::Conversation), sel, "the selection stayed");
    assert_eq!(s.d.pane(PaneKind::Conversation).unwrap().messages().len(), loaded);
}

#[test]
fn in_a_split_pane_the_wheel_up_to_the_top_asks_for_one_older_page() {
    let mut s = big();
    s.keys("space w v k");
    s.d.screen();
    let sel = chosen(&s.d, PaneKind::Conversation);
    for _ in 0..400 {
        wheel(&mut s.d, PaneKind::Conversation, true, 1);
        s.d.now += Duration::from_millis(100);
        s.pump();
    }
    assert_eq!(older(&s), 1);
    assert_eq!(chosen(&s.d, PaneKind::Conversation), sel, "the selection stayed");
}

#[test]
fn a_live_message_does_not_move_a_view_the_wheel_moved() {
    let mut s = big();
    let c = s.d.app.model.conversations().iter().find(|c| c.name == "big-history").unwrap().clone();
    let me = s.d.app.model.me(&c.workspace).unwrap().clone();
    let post = |s: &mut Scripted, ts: u64| {
        let message = Message {
            ts: Ts(ts),
            user: me.clone(),
            text: Remote::new("fresh from the wire"),
            thread: None,
            reactions: Vec::new(),
            edited: false,
            files: Vec::new(),
        };
        let live = Live::Message {
            workspace: c.workspace.clone(),
            conversation: c.id.clone(),
            thread: None,
            broadcast: false,
            changed: false,
            mention: false,
            message,
            users: Vec::new(),
        };
        s.b.push(Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) });
        s.pump();
    };
    let newest = |s: &Scripted| s.d.pane(PaneKind::Conversation).unwrap().messages().last().unwrap().ts;
    wheel(&mut s.d, PaneKind::Conversation, true, 5);
    let before = view(&s.d, PaneKind::Conversation);
    for i in 1..=3 {
        let ts = newest(&s).0 + i;
        post(&mut s, ts);
    }
    let screen = s.d.screen();
    assert_eq!(view(&s.d, PaneKind::Conversation), before, "nothing moved on screen");
    assert!(screen.contains("3 new"), "{screen}");
    // Down to the end: the newest follow again.
    wheel(&mut s.d, PaneKind::Conversation, false, 50);
    let ts = newest(&s).0 + 1;
    post(&mut s, ts);
    s.d.screen();
    assert!(shows(&s.d, PaneKind::Conversation, Ts(ts)), "at the bottom, new messages stay in view");
}
