//! Answers from the local cache over a scripted backend: an earlier run's workspaces and newest
//! messages draw before the backend answers, are marked as such in the status line, and give way
//! to the backend's own answer — or stay, still marked, when that answer fails. A cached answer
//! never ends the wait for the real one, and never replaces something the backend already gave.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, FailKind, Generation, Page, Snapshot};
use slakio_core::model::Message;
use slakio_core::sanitize::Remote;
use slakio_world::World;

/// The demo world's snapshot with its first workspace renamed, so the cache's is told apart.
fn earlier_snapshot() -> Snapshot {
    let mut s = World::demo().snapshot().clone();
    s.conversations.iter_mut().find(|c| c.name.unsanitized() == "backend").unwrap().name = Remote::new("backend-old");
    s
}

/// Connected, the boot request held, the cache's snapshot handed over first.
fn cached_boot() -> Scripted {
    let mut s = Scripted::connected(true);
    s.pump();
    assert_eq!(s.b.held.len(), 1);
    let id = s.b.held[0].id;
    s.b.push(Envelope { generation: id, event: Event::CachedBoot(Box::new(earlier_snapshot())) });
    assert!(s.pump(), "the cache's answer draws");
    s
}

#[test]
fn a_cached_boot_draws_at_once_marked_cached_until_the_backends_own_replaces_it() {
    let mut s = cached_boot();
    let screen = s.d.screen();
    assert!(screen.contains("backend-old") && !screen.contains("Loading"), "{screen}");
    assert!(s.d.status_line().contains("read only · cached"), "{}", s.d.status_line());
    assert_eq!(s.b.held.len(), 1, "the boot still waits for the backend");
    s.b.answer(0);
    assert!(s.pump());
    let screen = s.d.screen();
    assert!(!screen.contains("backend-old") && screen.contains("backend"), "{screen}");
    let line = s.d.status_line();
    assert!(line.contains("read only") && !line.contains("cached"), "{line}");
}

#[test]
fn a_cached_boot_after_the_backends_own_is_dropped() {
    let mut s = Scripted::booted(true, None);
    let before = s.d.screen();
    let id = s.b.log[0].id;
    s.b.push(Envelope { generation: id, event: Event::CachedBoot(Box::new(earlier_snapshot())) });
    assert!(!s.pump(), "never over what the backend gave");
    assert_eq!(s.d.screen(), before);
    assert!(!s.d.status_line().contains("cached"));
    // Nor for a request nobody waits for.
    s.b.push(Envelope { generation: Generation(id.0 + 50), event: Event::CachedBoot(Box::new(earlier_snapshot())) });
    assert!(!s.pump());
}

#[test]
fn a_failed_boot_keeps_the_cached_list_says_why_and_retries_on_r() {
    let mut s = cached_boot();
    s.b.fail(0, FailKind::Offline);
    assert!(s.pump());
    let screen = s.d.screen();
    assert!(screen.contains("backend-old"), "the earlier list stays: {screen}");
    assert!(!screen.contains("Could not load workspaces"), "{screen}");
    let line = s.d.status_line();
    assert!(line.contains("Offline") && line.contains("cached"), "{line}");
    s.keys("r");
    assert_eq!(s.b.held.len(), 1, "r asks for the workspaces again");
    assert!(matches!(s.b.held[0].command, Command::Boot));
}

/// Booted from the backend, `backend` opened: its newest page held.
fn opened() -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.list_cursor_on("backend");
    s.keys("enter");
    assert_eq!(s.b.held.len(), 1);
    s
}

/// The world's newest page for the held request, as an earlier run's cache would have it: the
/// newest message missing, the one before it as it read before an edit.
fn cached_page(s: &mut Scripted) -> (Envelope, Envelope) {
    let fresh = s.b.reply(0);
    let Event::History(page) = &fresh.event else { panic!("a page") };
    let mut messages: Vec<Message> = page.messages[..page.messages.len() - 1].to_vec();
    let last = messages.last_mut().unwrap();
    last.text = Remote::new("as it read in the last run");
    let cached = Page { messages, complete: false, ..(**page).clone() };
    (Envelope { generation: fresh.generation, event: Event::CachedHistory(Box::new(cached)) }, fresh)
}

#[test]
fn a_cached_page_draws_while_waiting_and_the_backends_page_replaces_it() {
    let mut s = opened();
    let (cached, fresh) = cached_page(&mut s);
    s.b.push(cached);
    assert!(s.pump());
    let screen = s.d.screen();
    assert!(screen.contains("as it read in the last run") && !screen.contains("Loading messages"), "{screen}");
    assert!(s.d.status_line().contains("cached"), "{}", s.d.status_line());
    let Event::History(page) = &fresh.event else { unreachable!() };
    let newest = page.messages.last().unwrap().text.unsanitized().to_string();
    s.b.push(fresh.clone());
    assert!(s.pump());
    let screen = s.d.screen();
    assert!(!screen.contains("as it read in the last run"), "the backend's page replaces the cache's: {screen}");
    assert!(screen.contains(newest.split_whitespace().next().unwrap()), "{screen}");
    assert!(!s.d.status_line().contains("cached"), "{}", s.d.status_line());
}

#[test]
fn a_cached_page_never_lands_over_loaded_messages_or_for_another_request() {
    let mut s = opened();
    let (cached, fresh) = cached_page(&mut s);
    s.b.push(fresh);
    assert!(s.pump());
    let before = s.d.screen();
    s.b.push(cached.clone());
    assert!(!s.pump(), "after the backend's page");
    let other = Envelope { generation: Generation(cached.generation.0 + 9), ..cached };
    s.b.push(other);
    assert!(!s.pump(), "a request nobody waits for");
    assert_eq!(s.d.screen(), before);
}

#[test]
fn a_failed_page_keeps_the_cached_messages_and_a_retry_asks_for_the_newest_again() {
    let mut s = opened();
    let (cached, _) = cached_page(&mut s);
    s.b.push(cached);
    s.pump();
    s.b.push(Envelope::failed(s.b.log.last().unwrap().id, FailKind::Network, "scripted"));
    assert!(s.pump());
    let screen = s.d.screen();
    assert!(screen.contains("as it read in the last run"), "{screen}");
    assert!(!screen.contains("Could not load messages"), "{screen}");
    assert!(s.d.status_line().contains("cached"));
    // Moving to the oldest cached message asks for nothing older while the cache's are shown.
    s.keys("g g");
    assert!(s.b.held.is_empty(), "{:?}", s.b.held);
    s.keys("r");
    assert_eq!(s.b.held.len(), 1);
    assert!(
        matches!(s.b.held[0].command, Command::History { before: None, .. }),
        "the newest page again, not an older one: {:?}",
        s.b.held[0].command
    );
}

#[test]
fn at_80_columns_a_notice_and_the_cached_mark_leave_the_open_conversations_name_whole() {
    let mut s = cached_boot();
    s.d.app.resize(80, 24);
    s.b.fail(0, FailKind::Offline);
    s.pump();
    s.d.list_cursor_on("backend-old");
    s.keys("enter");
    let (cached, _) = cached_page(&mut s);
    s.b.push(cached);
    s.pump();
    let line = s.d.status_line();
    assert!(line.contains("Offline") && line.contains("cached"), "{line}");
    assert!(line.contains("#backend-old"), "the place is not cut while the line has room: {line}");
}

// Every Korean cache notice fits 40 cells (two cells a syllable made the first ones too long to
// read whole in the status line's message slot).
#[test]
fn every_korean_cache_notice_fits_forty_cells() {
    use slakio_core::i18n::{I18n, Label, Lang};
    let notices = [
        Label::CacheRebuilt,
        Label::CacheReadOnly,
        Label::CacheInUse,
        Label::CacheFailed,
        Label::CacheBackupFailed,
    ];
    for lang in [Lang::Ko] {
        let i18n = I18n::new(lang);
        for l in notices {
            let text = i18n.label(l);
            let cells = unicode_width::UnicodeWidthStr::width(&*text);
            assert!(cells <= 40, "{lang:?} {l:?}: {cells} cells: {text}");
        }
    }
}
