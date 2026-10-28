//! What a real workspace says about its conversations and people, drawn: a muted conversation is
//! faint, a muted channel badged (and counted) for its mentions only, a muted DM never; the user in do not disturb has
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
        // Muted: its mentions of the user only, never its plain unread count.
        let alerts = row_of(&screen, "alerts");
        assert!(alerts.contains(" 4 ") && !alerts.contains('9'), "{at}");
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

/// A muted channel counts its mentions of the user (Home, Activity), never its plain unread
/// messages; a muted DM counts for nothing (every message of a DM is a mention in Slack's counts).
#[test]
fn a_muted_channel_counts_its_mentions_only_and_a_muted_dm_nothing() {
    let mut muted = world();
    let ws = muted.workspaces[0].id.clone();
    let dm = muted.conversations.iter().position(|c| c.workspace == ws && c.is_dm() && c.unread > 0).unwrap();
    (muted.conversations[dm].unread, muted.conversations[dm].mentions) = (3, 3);
    let mut loud = muted.clone();
    loud.conversations.iter_mut().find(|c| c.name == "alerts" && c.workspace == ws).unwrap().muted = false;
    muted.conversations[dm].muted = true;
    let (s, s2) = (Scripted::booted(true, Some(muted)), Scripted::booted(true, Some(loud)));
    let (home, home2) = (s.d.app.view_unread(0, View::Home), s2.d.app.view_unread(0, View::Home));
    assert_eq!(home.mentions, home2.mentions, "the muted channel's mentions count");
    let (dms, dms2) = (s.d.app.view_unread(0, View::Dms), s2.d.app.view_unread(0, View::Dms));
    assert_eq!(dms.dms + 3, dms2.dms, "the muted DM counts for nothing");
    assert_eq!(s.d.app.model.mentions() + 3, s2.d.app.model.mentions(), "in Activity: the DM's 3 only");
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
    // With two workspaces, it says whose, in that workspace's stripe (found by its id).
    let notice = s.d.app.status.notice(s.d.now).expect("the notice");
    assert_eq!(notice.about.as_ref(), Some(&ws));
    let name = World::demo().snapshot().workspaces[0].name.unsanitized().to_string();
    assert!(matches!(&notice.msg, slakio_core::i18n::Msg::FailWorkspace { workspace, .. } if *workspace == name));
    // At 80 columns it fits whole: the short words, the name kept.
    s.d.app.resize(80, 24);
    let line = s.d.status_line();
    assert!(line.contains(&format!("{name}: Unread counts not loaded")), "{line}");
    assert!(!line.contains("may be old") && !line.contains('…'), "{line}");
}

/// A first start with no counts kept and Slack's counts failing: the words say they are not
/// loaded (nothing "shown may be old"), and the views show `?`, faint, not a look of all read;
/// it stays after the notice is gone.
#[test]
fn counts_unknown_on_a_first_start_show_as_unknown() {
    let mut snap = World::demo().snapshot().clone();
    snap.counts_failed = true;
    for c in &mut snap.conversations {
        (c.unread, c.mentions) = (0, 0);
    }
    let mut s = Scripted::booted(true, Some(snap));
    let line = s.d.status_line();
    assert!(line.contains("Unread counts not loaded") && !line.contains("may be old"), "{line}");
    s.d.now += std::time::Duration::from_secs(30);
    s.d.app.on_tick(s.d.now);
    let screen = s.d.screen();
    let home = screen.lines().find(|l| l.contains("Home")).unwrap_or_else(|| panic!("{screen}"));
    assert!(home.split("││").next().unwrap().trim_end().ends_with('?'), "{screen}");
    let (y, line) = screen.lines().enumerate().find(|(_, l)| l.contains("Home")).unwrap();
    let x = line
        .chars()
        .take_while(|c| *c != '?')
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum::<usize>();
    assert_eq!(s.d.buffer()[(x as u16, y as u16)].style().fg, s.d.app.theme.faint().fg);
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

/// A thread the backend read only in part says so after the last reply read, faint, and the
/// bots among the senders are marked `app`.
#[test]
fn a_thread_read_only_in_part_says_so_and_a_bot_is_marked() {
    for (w, h) in [(120, 40), (80, 24)] {
        thread_cut_at(w, h);
    }
}

/// The cut thread at `w`x`h`: the whole notice is on screen, however narrow the thread pane.
fn thread_cut_at(w: u16, h: u16) {
    use slakio_core::model::Target;
    let mut s = Scripted::booted(true, None);
    s.d.app.resize(w, h);
    s.d.list_cursor_on("long-threads");
    s.keys("enter");
    let i = s.b.held.len() - 1;
    s.b.answer(i);
    s.pump();
    s.keys("g g enter");
    let i =
        s.b.held
            .iter()
            .position(|r| matches!(&r.command, Command::History { target: Target::Thread { .. }, .. }))
            .expect("the thread asked for");
    let mut e = s.b.reply(i);
    let Event::History(page) = &mut e.event else { panic!("{e:?}") };
    page.cut = true;
    // Its newest reply is a bot's, posting as a person's name.
    let last = page.messages.last_mut().unwrap();
    last.user = slakio_core::model::UserId::new("BHOOK~1");
    page.users.push(slakio_core::model::User {
        id: last.user.clone(),
        workspace: page.target.workspace().clone(),
        name: "Minsu Kim".into(),
        display_name: "Minsu Kim".into(),
        org: slakio_core::model::Org::Own,
        bot: true,
        presence: Presence::Unknown,
    });
    s.b.push(e);
    s.pump();
    let screen = s.d.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let notice = lines.iter().position(|l| l.contains("Newer replies")).unwrap_or_else(|| panic!("{screen}"));
    let words: String = lines[notice..]
        .iter()
        .take(2)
        .map(|l| l.split('│').filter(|c| c.contains("Newer") || c.contains("loaded")).collect::<String>())
        .collect();
    assert!(words.contains("Newer replies") && words.contains("not loaded"), "the whole notice at {w}x{h}: {screen}");
    let bot = lines.iter().position(|l| l.contains("Minsu Kim app")).unwrap_or_else(|| panic!("{screen}"));
    assert!(bot < notice, "the notice after the last reply (the bot's): {screen}");
}

/// In the compact column a long name keeps eight cells at least: `ext` goes before the name is
/// cut shorter, the bot's `app` stays; the tags are faint and italic, a style no name is drawn
/// in (also without color).
#[test]
fn compact_tags_leave_the_name_room_and_read_apart_from_a_name() {
    use slakio_core::model::{Org, User, UserId};
    use slakio_core::sanitize::Remote;
    for theme in [slakio_tui::theme::Theme::terminal(), slakio_tui::theme::Theme::no_color()] {
        let mut s = Scripted::booted(true, None);
        s.d.app.theme = theme;
        s.d.app.settings.compact = true;
        s.d.list_cursor_on("deploys");
        s.keys("enter");
        let i = s.b.held.len() - 1;
        let mut e = s.b.reply(i);
        let Event::History(page) = &mut e.event else { panic!("{e:?}") };
        let last = page.messages.last_mut().unwrap();
        last.user = UserId::new("BREL~1");
        page.users.push(User {
            id: last.user.clone(),
            workspace: page.target.workspace().clone(),
            name: Remote::new("Partner Release Notifier Bot".to_string()),
            display_name: Remote::new("Partner Release Notifier Bot".to_string()),
            org: Org::External(Remote::new("TPARTNER".to_string())),
            bot: true,
            presence: Presence::Unknown,
        });
        s.b.push(e);
        s.pump();
        let screen = s.d.screen();
        let line = screen.lines().find(|l| l.contains("Partner")).unwrap_or_else(|| panic!("{screen}"));
        let rest = line.split("Partner").nth(1).unwrap().split('…').next().unwrap();
        assert!("Partner".len() + rest.chars().count() + 1 >= 8, "the name keeps 8 cells: {line}");
        assert!(!line.contains("app ext"), "ext went first: {line}");
        assert!(line.contains("… app"), "the bot's mark stays: {line}");
        let (y, row) = screen.lines().enumerate().find(|(_, l)| l.contains("… app")).unwrap();
        let x = row[..row.find("… app").unwrap()].chars().count() + 2;
        let style = s.d.buffer()[(x as u16, y as u16)].style();
        assert!(style.add_modifier.contains(ratatui::style::Modifier::ITALIC), "{row}");
        assert!(!style.add_modifier.contains(ratatui::style::Modifier::BOLD), "{row}");
    }
}
