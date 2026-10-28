//! Reading back through a long history without spending the request budget: `gg` selects the
//! oldest message loaded and asks for one more page, never a burst; older pages go out paced
//! however many gestures want them; one on its way shows a loading row, one that failed a row
//! with the key that asks again (`r`, or the next gesture), whatever Slack said but for the
//! sign-in, and one Slack asked to slow down for is asked again by itself after Slack's pause
//! (nothing offers `r` meanwhile); the true start shows a start row. In the 10,000-message
//! channel. A thread's older pages come from what was read of it: not paced, and `gg` goes
//! straight to the thread's own message.

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
    // The request limit stopped it: the row says so, and the next gesture asks again, once.
    let i = older(&s)[0];
    s.b.fail(i, FailKind::CapReached);
    s.pump();
    let screen = s.d.screen();
    assert!(screen.contains("Older messages not loaded · r retry"), "{screen}");
    s.d.now += Duration::from_secs(5);
    s.keys("g g k k");
    assert_eq!(older(&s).len(), 1, "gg asks for the failed page again");
    // Slack refuses it with an error of its own: still a row with `r`, and `r` asks again.
    let i = older(&s)[0];
    s.b.fail(i, FailKind::Api { code: "internal_error".into() });
    s.pump();
    let screen = s.d.screen();
    assert!(screen.contains("Older messages not loaded · r retry"), "{screen}");
    assert!(s.d.status_line().contains("r retry"), "{}", s.d.status_line());
    s.d.now += Duration::from_secs(5);
    s.keys("r");
    assert_eq!(older(&s).len(), 1, "r asks for the older page again, once");
    assert!(!s.d.status_line().contains("Nothing to retry"), "{}", s.d.status_line());
    let n = loaded(&s);
    answer_older(&mut s);
    assert!(loaded(&s) > n, "and it arrives");
    assert!(!s.d.screen().contains("Older messages not loaded"), "{}", s.d.screen());
}

#[test]
fn an_older_page_slack_refused_with_any_error_but_the_sign_in_can_be_asked_again() {
    for code in ["internal_error", "fatal_error", "request_timeout", "service_unavailable", "channel_not_found"] {
        let mut s = big();
        s.keys("g g");
        let i = older(&s)[0];
        s.b.fail(i, FailKind::Api { code: code.into() });
        s.pump();
        assert!(s.d.screen().contains("Older messages not loaded · r retry"), "{code}: {}", s.d.screen());
        s.d.now += Duration::from_secs(2);
        s.keys(": r e t r y enter");
        assert_eq!(older(&s).len(), 1, "{code}: :retry asks again");
        answer_older(&mut s);
    }
    for kind in [FailKind::Network, FailKind::Offline, FailKind::Busy] {
        let mut s = big();
        s.keys("g g");
        let i = older(&s)[0];
        s.b.fail(i, kind.clone());
        s.pump();
        s.d.now += Duration::from_secs(2);
        s.keys("k");
        assert_eq!(older(&s).len(), 1, "{kind:?}: k at the top asks again");
    }
    // Signed out: asking again cannot help, and nothing offers it.
    let mut s = big();
    s.keys("g g");
    let i = older(&s)[0];
    s.b.fail(i, FailKind::Auth);
    s.pump();
    let screen = s.d.screen();
    assert!(screen.contains("Older messages not loaded") && !screen.contains("r retry"), "{screen}");
    s.d.now += Duration::from_secs(2);
    s.keys("g g r");
    assert!(older(&s).is_empty(), "nothing is asked again");
}

#[test]
fn an_older_page_slack_asked_to_slow_down_for_is_asked_again_by_itself() {
    let mut s = big();
    s.keys("g g");
    let i = older(&s)[0];
    s.b.fail(i, FailKind::RateLimited { retry_after: Some(Duration::from_secs(20)) });
    s.pump();
    // While Slack's pause runs, nothing offers `r`: the row says the page is asked for again.
    let screen = s.d.screen();
    assert!(screen.contains("Older messages not loaded · trying again shortly"), "{screen}");
    assert!(!screen.contains("r retry"), "{screen}");
    assert!(!s.d.status_line().contains("r retry"), "{}", s.d.status_line());
    s.keys("r g g");
    assert!(older(&s).is_empty(), "r and gg wait for Slack's pause too");
    assert!(s.d.status_line().contains("Slack asked to slow down"), "{}", s.d.status_line());
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

#[test]
fn gg_in_a_thread_read_whole_goes_to_its_own_message_at_once() {
    let mut d = demo::Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    let thread = || PaneKind::Thread;
    assert!(!d.pane(thread()).unwrap().complete(), "the newest replies first");
    // One `gg`, no time passing: every older page comes at once, from what was read.
    d.keys("g g");
    let pane = d.pane(thread()).unwrap();
    assert!(pane.complete(), "the thread's start is loaded");
    assert_eq!(pane.messages().len(), 1_201, "its own message and every reply");
    assert_eq!(pane.selected(), Some(0), "its own message is selected");
    let screen = d.screen();
    assert!(screen.contains("─ 1,200 replies ─"), "the rule under it: {screen}");
    assert!(!screen.contains("Loading older"), "{screen}");
    assert!(d.app.deadline().is_none(), "nothing waits for its turn");
}
