//! What a real workspace says about its conversations and people, drawn: a muted conversation is
//! faint and never badged (nor counted by its section or view), the user in do not disturb has
//! `◐` in the status line, a Slack Connect channel its `ext` and a sender of another organization
//! `ext` after the name; counts Slack could not tell keep those shown before, and the status line
//! says so. At 120x40 and 80x24.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, Snapshot};
use slakio_core::model::Presence;
use slakio_tui::app::shell::View;
use slakio_world::World;

/// The world's snapshot with the user in do not disturb and `alerts` muted though it mentions
/// the user.
fn world() -> Snapshot {
    let mut s = World::demo().snapshot().clone();
    let me = s.workspaces[0].me.clone();
    let ws = s.workspaces[0].id.clone();
    s.users.iter_mut().find(|u| u.id == me && u.workspace == ws).unwrap().presence = Presence::Dnd;
    let alerts = s.conversations.iter_mut().find(|c| c.name == "alerts" && c.workspace == ws).unwrap();
    (alerts.muted, alerts.unread, alerts.mentions) = (true, 9, 4);
    s
}

fn row_of<'a>(screen: &'a str, name: &str) -> &'a str {
    screen
        .lines()
        .find(|l| l.contains(&format!("# {name} ")) || l.ends_with(&format!("# {name}")))
        .unwrap_or_else(|| panic!("{name}: {screen}"))
}

#[test]
fn mute_do_not_disturb_and_slack_connect_are_drawn_at_120x40_and_80x24() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut s = Scripted::booted(true, Some(world()));
        s.d.app.resize(w, h);
        s.d.list_cursor_on("partner-shared");
        s.keys("enter");
        s.b.answer(0);
        s.pump();
        let screen = s.d.screen();
        let at = format!("{w}x{h}: {screen}");
        // Muted: no pill, though it mentions the user.
        let alerts = row_of(&screen, "alerts");
        assert!(!alerts.contains('4') && !alerts.contains('9'), "{at}");
        assert!(row_of(&screen, "partner-shared").contains("ext"), "{at}");
        assert!(s.d.status_line().contains('◐'), "the user is in do not disturb: {at}");
        if w >= 100 {
            assert!(s.d.status_line().contains("◐ do not disturb"), "{at}");
        }
        // A partner's message: `ext` after the name.
        let partner = screen.lines().any(|l| l.contains(" ext  ") && l.contains("││"));
        assert!(partner, "a sender of another organization is marked: {at}");
        insta::assert_snapshot!(format!("reading_{w}x{h}"), s.d.snap());
    }
}

#[test]
fn a_muted_conversation_adds_to_no_count() {
    let mut muted = world();
    let s = Scripted::booted(true, Some(muted.clone()));
    let home = s.d.app.view_unread(0, View::Home);
    let ws = muted.workspaces[0].id.clone();
    muted.conversations.iter_mut().find(|c| c.name == "alerts" && c.workspace == ws).unwrap().muted = false;
    let s2 = Scripted::booted(true, Some(muted));
    assert_eq!(s2.d.app.view_unread(0, View::Home).mentions, home.mentions + 4, "only the unmuted one counts");
    assert_eq!(s.d.app.model.mentions() + 4, s2.d.app.model.mentions(), "nor in Activity");
}

#[test]
fn counts_slack_could_not_tell_keep_those_shown_and_say_so() {
    let mut s = Scripted::connected(true);
    s.d.app.connect_workspaces(s.b.caps, &signed());
    s.pump();
    let ws = World::demo().snapshot().workspaces[0].id.clone();
    let boot_of = |s: &Scripted| {
        s.b.held.iter().position(|r| r.command == Command::Boot && r.workspace.as_ref() == Some(&ws)).unwrap()
    };
    let i = boot_of(&s);
    let first = s.b.reply(i);
    s.b.push(first.clone());
    s.pump();
    let before = s.d.app.view_unread(0, View::Home);
    assert!(before.mentions > 0);
    // Its answer again (as a later boot's would be); this time Slack's counts failed: zeros
    // that mean "unknown".
    let Event::Booted(snap) = first.event else { panic!() };
    let mut failed = snap.only(&ws);
    failed.counts_failed = true;
    for c in &mut failed.conversations {
        (c.unread, c.mentions) = (0, 0);
    }
    s.b.push(Envelope { generation: first.generation, event: Event::Booted(Box::new(failed)) });
    s.pump();
    assert_eq!(s.d.app.view_unread(0, View::Home), before, "the counts shown before stay");
    assert!(s.d.status_line().contains("Unread counts not loaded"), "{}", s.d.status_line());
}

fn signed() -> Vec<slakio_tui::app::model::SignedIn> {
    World::demo()
        .snapshot()
        .workspaces
        .iter()
        .map(|w| slakio_tui::app::model::SignedIn {
            id: w.id.clone(),
            name: w.name.unsanitized().to_string(),
            read_only: true,
        })
        .collect()
}
