//! Reading back through a long history without spending the request budget: `gg` selects the
//! oldest message loaded and asks for one more page, never a burst; older pages go out paced
//! however many gestures want them; one on its way shows a loading row, one that failed a row
//! with the key that asks again (`r`), and one Slack asked to slow down for is asked again by
//! itself after Slack's pause; the true start shows a start row. In the 10,000-message channel.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, FailKind};
use slakio_tui::app::PaneKind;
use std::time::Duration;

/// The held requests for an older page.
fn older(s: &Scripted) -> Vec<usize> {
    let held = s.b.held.iter().enumerate();
    held.filter(|(_, r)| matches!(r.command, Command::History { before: Some(_), .. })).map(|(i, _)| i).collect()
}

/// Every older page asked for so far, answered or not.
fn older_asked(s: &Scripted) -> usize {
    let log = s.b.log.iter().filter(|r| matches!(r.command, Command::History { before: Some(_), .. })).count();
    log + older(s).len()
}

/// Booted, `#big-history` open with its newest page.
fn big() -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.list_cursor_on("big-history");
    s.keys("enter");
    let i = s.b.held.len() - 1;
    s.b.answer(i);
    s.pump();
    s
}

fn loaded(s: &Scripted) -> usize {
    s.d.pane(PaneKind::Conversation).unwrap().messages().len()
}

/// Answer every older page held, then one turn of the loop.
fn answer_older(s: &mut Scripted) {
    while let Some(&i) = older(s).first() {
        s.b.answer(i);
    }
    s.pump();
}

#[test]
fn gg_again_and_again_asks_one_page_at_a_time_and_paced() {
    let mut s = big();
    let first = loaded(&s);
    // Fifty `gg` at once, every page answered at once: one page, not fifty.
    for _ in 0..50 {
        s.keys("g g");
        answer_older(&mut s);
    }
    assert_eq!(older_asked(&s), 1, "one page for fifty gestures within the pace");
    assert_eq!(loaded(&s), first + 200);
    // A minute of `gg` every 100 ms: at most one page per pace step, never a burst.
    for _ in 0..600 {
        s.d.now += Duration::from_millis(100);
        s.keys("g g");
        answer_older(&mut s);
    }
    let asked = older_asked(&s);
    assert!((30..=41).contains(&asked), "paced: {asked} pages in a minute");
    // A page that lands asks for nothing more by itself.
    s.d.now += Duration::from_secs(10);
    s.pump();
    assert_eq!(older_asked(&s), asked, "no gesture, no page");
}

#[test]
fn an_older_page_on_its_way_shows_a_row_and_one_that_failed_can_be_retried() {
    let mut s = big();
    s.keys("g g");
    assert_eq!(older(&s).len(), 1);
    assert!(s.d.screen().contains("Loading older messages…"), "{}", s.d.screen());
    // The request limit stopped it: the row says so and `r` asks again.
    let i = older(&s)[0];
    s.b.fail(i, FailKind::CapReached);
    s.pump();
    let screen = s.d.screen();
    assert!(screen.contains("Older messages not loaded · r retry"), "{screen}");
    s.d.now += Duration::from_secs(5);
    s.keys("g g k k");
    assert!(older(&s).is_empty(), "a failed page waits for a retry");
    s.keys("r");
    assert_eq!(older(&s).len(), 1, "r asks for the older page again");
    assert!(!s.d.status_line().contains("Nothing to retry"), "{}", s.d.status_line());
    let n = loaded(&s);
    answer_older(&mut s);
    assert!(loaded(&s) > n, "and it arrives");
}

#[test]
fn an_older_page_slack_asked_to_slow_down_for_is_asked_again_by_itself() {
    let mut s = big();
    s.keys("g g");
    let i = older(&s)[0];
    s.b.fail(i, FailKind::RateLimited { retry_after: Some(Duration::from_secs(20)) });
    s.pump();
    assert!(s.d.screen().contains("Older messages not loaded"), "{}", s.d.screen());
    let at = s.d.now;
    assert!(s.d.app.deadline().is_some(), "the loop wakes up for it");
    for _ in 0..19 {
        s.d.now += Duration::from_secs(1);
        s.d.app.on_tick(s.d.now);
        s.pump();
        assert!(older(&s).is_empty(), "not before Slack's pause");
    }
    s.d.now = at + Duration::from_secs(20);
    assert!(s.d.app.on_tick(s.d.now), "a redraw");
    s.pump();
    assert_eq!(older(&s).len(), 1, "asked again by itself");
    answer_older(&mut s);
    assert!(!s.d.screen().contains("Older messages not loaded"));
}

#[test]
fn the_start_of_a_history_read_back_is_marked() {
    let mut s = big();
    let mut presses = 0;
    while !s.d.pane(PaneKind::Conversation).unwrap().complete() {
        s.d.now += Duration::from_secs(2);
        s.keys("g g");
        answer_older(&mut s);
        presses += 1;
        assert!(presses < 60);
    }
    s.keys("g g");
    assert_eq!(loaded(&s), 10_000);
    let screen = s.d.screen();
    assert!(screen.contains("Start of the conversation"), "{screen}");
    assert!(!screen.contains("Loading older"), "{screen}");
}
