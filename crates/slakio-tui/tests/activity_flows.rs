//! Activity in the app: read when it shows (each workspace's newest page, not again within a
//! moment), merged newest first with each workspace's stripe, two rows a notice (its mark,
//! where, when; who and the text), unread ones bold; `Enter` opens the message (a thread's reply
//! beside its conversation), `l` stays in the list; the next page as the cursor nears the end;
//! a mention that comes live is listed at once and Activity read again once (not per message);
//! so too after the live updates come back; a page that failed says so and `r` asks again;
//! hostile text drawn harmless. At 120x40 and 80x24, in English and Korean.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use demo::{Demo, assert_harmless, mask_hangul};
use ratatui::style::Modifier;
use scripted::Scripted;
use slakio_core::backend::{Command, Connection, Envelope, Event, FailKind, Generation, Live, NoticeKind};
use slakio_core::i18n::{I18n, Label, Lang};
use slakio_core::model::{Message, Ts, WorkspaceId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::PaneKind;
use slakio_tui::app::activity::{FRESH, SETTLE};
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::View;
use slakio_world::{World, names};
use std::time::Duration;

/// The Activity requests the scripted backend holds.
fn asked(s: &Scripted) -> Vec<(Option<WorkspaceId>, Option<String>)> {
    (s.b.held.iter())
        .filter_map(|r| match &r.command {
            Command::Activity { cursor } => Some((r.workspace.clone(), cursor.clone())),
            _ => None,
        })
        .collect()
}

/// Answer every Activity request held.
fn answer_all(s: &mut Scripted) {
    while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
        s.b.answer(i);
        s.pump();
    }
}

fn push(s: &mut Scripted, live: Live) {
    s.b.push(Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) });
    s.pump();
}

/// Booted over a scripted backend, Activity shown and its first pages answered.
fn shown(w: u16, h: u16, lang: Lang) -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.app.i18n = I18n::new(lang);
    s.d.app.resize(w, h);
    assert!(asked(&s).is_empty(), "nothing is read for Activity before it shows");
    s.keys("space a");
    assert_eq!(s.d.app.view(), View::Activity);
    let wanted = asked(&s);
    assert_eq!(wanted.len(), 2, "each workspace's newest page: {wanted:?}");
    assert!(wanted.iter().all(|(ws, cursor)| ws.is_some() && cursor.is_none()));
    answer_all(&mut s);
    s
}

#[test]
fn activity_lists_both_workspaces_newest_first_and_says_what_each_notice_is() {
    for (w, h, lang) in [(120, 40, Lang::En), (80, 24, Lang::En), (80, 24, Lang::Ko)] {
        let s = shown(w, h, lang);
        let screen = s.d.screen();
        assert!(!screen.contains("comes in a"), "no placeholder: {screen}");
        assert!(screen.contains("#incidents") || screen.contains("#side-project"), "{screen}");
        let rows = s.d.app.list_rows();
        let notices = rows.iter().filter(|r| matches!(r, Row::Notice(_))).count();
        let world = World::demo();
        let want: usize = (world.snapshot().workspaces.iter()).map(|ws| world.activity(&ws.id).len().min(20)).sum();
        assert_eq!(notices, want, "each workspace's first page");
        let at: Vec<Ts> =
            (0..s.d.app.model.notices.len()).map(|i| s.d.app.model.notices.get(i).unwrap().1.at).collect();
        assert!(at.windows(2).all(|p| p[0] >= p[1]), "newest first");
        let workspaces: std::collections::HashSet<&WorkspaceId> =
            (0..s.d.app.model.notices.len()).map(|i| &s.d.app.model.notices.get(i).unwrap().0).collect();
        assert_eq!(workspaces.len(), 2, "merged");
        assert!(screen.contains('▌'), "each notice's workspace stripe");
        if (w, lang) == (120, Lang::En) {
            insta::assert_snapshot!("activity_120x40", mask_hangul(&screen));
        }
        if lang == Lang::Ko {
            insta::assert_snapshot!("activity_ko_80x24", mask_hangul(&screen));
        }
    }
}

#[test]
fn an_unread_notice_is_bold_and_a_read_one_is_not() {
    let s = shown(120, 40, Lang::En);
    let buf = s.d.buffer();
    let rows = s.d.app.list_rows();
    let (mut seen_unread, mut seen_read) = (false, false);
    let top = 7; // the list's first row under the views and the rule
    for (k, row) in rows.iter().enumerate().take(20) {
        let Row::Notice(i) = row else { continue };
        let n = &s.d.app.model.notices.get(*i).unwrap().1;
        // The place's first cell, col 4 (inside the border, after the stripe and the mark).
        let cell = &buf[(4, top + k as u16)];
        if k == s.d.app.list_cursor() {
            continue;
        }
        assert_eq!(cell.modifier.contains(Modifier::BOLD), n.unread, "row {k} {}: {:?}", n.key, cell);
        if n.unread {
            seen_unread = true;
        } else {
            seen_read = true;
        }
    }
    assert!(seen_unread && seen_read);
}

#[test]
fn enter_opens_the_message_and_a_threads_reply_beside_it() {
    let mut s = shown(120, 40, Lang::En);
    let rows = s.d.app.list_rows();
    // A mention: its conversation, read around the message.
    let mention = rows
        .iter()
        .position(|r| {
            matches!(r, Row::Notice(i) if { let n = &s.d.app.model.notices.get(*i).unwrap().1; n.kind == NoticeKind::Mention && n.thread.is_none() })
        })
        .unwrap();
    let Row::Notice(i) = rows[mention] else { unreachable!() };
    let (ws, n) = s.d.app.model.notices.get(i).unwrap().clone();
    s.d.app.focus_list_row(mention);
    s.keys("enter");
    assert_eq!(s.d.focused_kind(), Some(PaneKind::Conversation));
    let around = s.b.held.iter().position(|r| matches!(&r.command, Command::Around { at, .. } if *at == n.ts));
    if let Some(at) = around {
        s.b.answer(at);
        s.pump();
    }
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    assert_eq!(pane.target().conversation(), &n.conversation);
    assert_eq!(pane.target().workspace(), &ws);
    let sel = pane.selected().map(|k| pane.messages()[k].ts);
    assert_eq!(sel, Some(n.ts), "the message selected");
    // A thread: its reply in the thread beside the conversation.
    s.keys("ctrl+h");
    s.keys("space a");
    let rows = s.d.app.list_rows();
    let thread = rows
        .iter()
        .position(
            |r| matches!(r, Row::Notice(i) if s.d.app.model.notices.get(*i).unwrap().1.kind == NoticeKind::Thread),
        )
        .expect("a thread notice");
    s.d.app.focus_list_row(thread);
    s.keys("enter");
    for _ in 0..4 {
        while let Some(i) = s.b.held.iter().position(|r| !matches!(r.command, Command::Activity { .. })) {
            s.b.answer(i);
            s.pump();
        }
    }
    assert_eq!(s.d.focused_kind(), Some(PaneKind::Thread), "the reply's thread has the keyboard");
}

#[test]
fn l_opens_staying_in_the_list() {
    let mut s = shown(120, 40, Lang::En);
    let first = s.d.app.list_rows().iter().position(|r| matches!(r, Row::Notice(_))).unwrap();
    s.d.app.focus_list_row(first);
    s.keys("l");
    assert!(s.d.app.focused_pane().is_none(), "the list keeps the keyboard");
    assert!(!s.d.app.open_panes().is_empty());
}

#[test]
fn the_next_page_comes_as_the_cursor_nears_the_end_one_at_a_time() {
    let mut s = shown(120, 40, Lang::En);
    let first = s.d.app.list_rows().iter().position(|r| matches!(r, Row::Notice(_))).unwrap();
    s.d.app.focus_list_row(first);
    assert!(asked(&s).is_empty(), "nothing more at the top");
    s.keys("G");
    let more = asked(&s);
    assert!(!more.is_empty() && more.iter().all(|(_, c)| c.as_deref() == Some("c20")), "{more:?}");
    s.keys("k j G");
    assert_eq!(asked(&s).len(), more.len(), "one page at a time");
    let shown_before = s.d.app.model.notices.len();
    answer_all(&mut s);
    assert!(s.d.app.model.notices.len() > shown_before);
    let screen = s.d.screen();
    assert!(asked(&s).is_empty(), "the cursor is far from the new end: {:?}", asked(&s));
    assert!(!screen.contains("Loading…"), "{screen}");
}

#[test]
fn a_live_mention_is_listed_at_once_and_activity_is_read_again_once() {
    let mut s = shown(120, 40, Lang::En);
    let ws = s.d.app.model.workspaces()[0].id.clone();
    let me = s.d.app.model.me(&ws).unwrap().clone();
    let c = s.d.app.model.conversations().iter().find(|c| c.workspace == ws && c.name == "deploys").unwrap().clone();
    let other =
        World::demo().snapshot().users.iter().find(|u| u.workspace == ws && u.id != me && !u.bot).unwrap().id.clone();
    let newest = s.d.app.model.notices.get(0).unwrap().1.at;
    for k in 0..5u64 {
        let message = Message {
            ts: Ts(newest.0 + 1_000_000 * (k + 1)),
            user: other.clone(),
            text: Remote::new(format!("ping <@{me}> {k}")),
            thread: None,
            reactions: Vec::new(),
            edited: false,
        };
        push(
            &mut s,
            Live::Message {
                workspace: ws.clone(),
                conversation: c.id.clone(),
                thread: None,
                broadcast: false,
                changed: false,
                mention: true,
                message,
                users: Vec::new(),
            },
        );
    }
    let top = &s.d.app.model.notices.get(0).unwrap().1;
    assert!(top.unread && top.conversation == c.id, "listed at once");
    assert!(s.d.screen().contains("#deploys"));
    assert!(asked(&s).is_empty(), "nothing asked per message");
    // Read again once things settle, not sooner than a while after it was read.
    s.d.now += SETTLE;
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert!(asked(&s).is_empty(), "not again within a moment of the last read");
    s.d.now += FRESH;
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert_eq!(asked(&s), [(Some(ws.clone()), None)], "once, for its workspace");
    answer_all(&mut s);
    assert!(
        !(0..s.d.app.model.notices.len()).any(|i| s.d.app.model.notices.get(i).unwrap().1.key.starts_with("live:"))
    );
    // The user's own message, or an edit, lists nothing.
    let before = s.d.app.model.notices.len();
    let mine = Message {
        ts: Ts(newest.0 + 99_000_000),
        user: me.clone(),
        text: Remote::new("me"),
        thread: None,
        reactions: Vec::new(),
        edited: false,
    };
    push(
        &mut s,
        Live::Message {
            workspace: ws.clone(),
            conversation: c.id.clone(),
            thread: None,
            broadcast: false,
            changed: false,
            mention: true,
            message: mine,
            users: Vec::new(),
        },
    );
    assert_eq!(s.d.app.model.notices.len(), before);
}

#[test]
fn after_the_live_updates_come_back_activity_is_read_again_and_not_while_it_hides() {
    let mut s = shown(120, 40, Lang::En);
    let ws = s.d.app.model.workspaces()[1].id.clone();
    s.d.now += FRESH;
    push(&mut s, Live::Connection { workspace: ws.clone(), state: Connection::Connected { catch_up: true } });
    s.d.now += SETTLE;
    s.d.app.on_tick(s.d.now);
    s.pump();
    let again: Vec<_> = asked(&s).into_iter().filter(|(w, _)| w.as_ref() == Some(&ws)).collect();
    assert_eq!(again, [(Some(ws.clone()), None)]);
    answer_all(&mut s);
    // Hidden: nothing is read, until it shows again.
    s.keys("space h");
    s.d.now += FRESH;
    push(&mut s, Live::Connection { workspace: ws.clone(), state: Connection::Connected { catch_up: true } });
    s.d.now += SETTLE + FRESH;
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert!(asked(&s).is_empty(), "not while Activity hides");
    s.keys("space a");
    assert!(asked(&s).iter().any(|(w, c)| w.as_ref() == Some(&ws) && c.is_none()));
}

#[test]
fn shown_again_within_a_moment_it_is_not_read_again() {
    let mut s = shown(120, 40, Lang::En);
    s.keys("space h");
    s.keys("space a");
    assert!(asked(&s).is_empty(), "read a moment ago");
    s.keys("space h");
    s.d.now += FRESH + Duration::from_millis(1);
    s.keys("space a");
    assert_eq!(asked(&s).len(), 2);
}

#[test]
fn a_page_that_failed_says_so_and_r_asks_again() {
    let mut s = Scripted::booted(true, None);
    s.keys("space a");
    let screen = s.d.screen();
    assert!(screen.contains("Loading…"), "{screen}");
    while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
        s.b.fail(i, FailKind::Network);
        s.pump();
    }
    let screen = s.d.screen();
    assert!(screen.contains("Could not load Activity") && screen.contains("r retry"), "{screen}");
    assert!(asked(&s).is_empty(), "never again by itself");
    s.keys("r");
    assert_eq!(asked(&s).len(), 2, "asked again");
    answer_all(&mut s);
    assert!(s.d.screen().contains('#'));
    // A later page that fails says so at the end.
    s.keys("G");
    while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
        s.b.fail(i, FailKind::RateLimited { retry_after: None });
        s.pump();
    }
    let list = s.d.app.list_rows().len();
    assert!(list > 0);
    let failed = s.d.app.activity_foot();
    assert!(matches!(failed, Some(slakio_tui::app::activity::Foot::Failed(_))), "{failed:?}");
}

#[test]
fn nothing_in_activity_says_so() {
    let mut s = Scripted::booted(true, None);
    s.keys("space a");
    while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
        let mut e = s.b.reply(i);
        if let Event::Activity(f) = &mut e.event {
            f.notices.clear();
            f.next = None;
        }
        s.b.push(e);
        s.pump();
    }
    let screen = s.d.screen();
    // Wrapped in the narrow list panel: its words, line by line.
    let empty = I18n::new(Lang::En).label(Label::ActivityEmpty).to_string();
    let words: String = screen.lines().map(|l| l.trim_matches(|c| c == '│' || c == ' ')).collect::<Vec<_>>().join(" ");
    assert!(words.contains(&empty), "{screen}");
}

#[test]
fn hostile_text_and_names_are_drawn_harmless() {
    let mut d = Demo::new(120, 40);
    d.keys("space a");
    d.pump();
    let hostile = d.app.model.conversations().iter().find(|c| c.name == names::HOSTILE).unwrap().id.clone();
    let at = (0..d.app.model.notices.len()).find(|i| d.app.model.notices.get(*i).unwrap().1.conversation == hostile);
    let at = at.expect("a notice in the hostile channel");
    let row = d.app.list_rows().iter().position(|r| *r == Row::Notice(at)).unwrap();
    d.app.focus_list_row(row);
    let screen = d.screen();
    assert_harmless(&screen);
    assert!(screen.contains(&format!("#{}", &names::HOSTILE[..9])), "{screen}");
}

#[test]
fn the_demo_lists_activity_and_files_still_says_later() {
    let mut d = Demo::new(80, 24);
    d.keys("space a");
    d.pump();
    assert!(d.app.model.notices.len() >= 20);
    assert!(!d.screen().contains("comes in a"));
    d.keys("space f");
    assert!(d.screen().contains("comes in a"));
}

#[test]
fn a_page_of_nothing_shown_asks_for_no_more_by_itself() {
    let mut s = shown(120, 40, Lang::En);
    s.keys("G");
    let more = asked(&s);
    assert!(!more.is_empty());
    while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
        let mut e = s.b.reply(i);
        if let Event::Activity(f) = &mut e.event {
            f.notices.clear();
            f.next = Some("c999".into());
        }
        s.b.push(e);
        s.pump();
    }
    assert!(asked(&s).is_empty(), "no run of empty pages read unasked");
    s.keys("k j");
    assert!(!asked(&s).is_empty(), "a key near the end asks again");
}

#[test]
fn t_opens_a_notice_in_a_new_tab_and_the_list_keeps_the_keyboard() {
    let mut s = shown(120, 40, Lang::En);
    let first = s.d.app.list_rows().iter().position(|r| matches!(r, Row::Notice(_))).unwrap();
    s.d.app.focus_list_row(first);
    let before = s.d.app.tab_titles().len();
    s.keys("t");
    assert_eq!(s.d.app.tab_titles().len(), before + 1, "{:?}", s.d.app.tab_titles());
}

/// Answer every Activity request held, each page changed by `change` first.
fn answer_with(s: &mut Scripted, change: impl Fn(&mut slakio_core::backend::Feed)) {
    while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
        let mut e = s.b.reply(i);
        if let Event::Activity(f) = &mut e.event {
            change(f);
        }
        s.b.push(e);
        s.pump();
    }
}

/// The keys of the notices listed, newest first.
fn listed(s: &Scripted) -> Vec<String> {
    (0..s.d.app.model.notices.len()).map(|i| s.d.app.model.notices.get(i).unwrap().1.key.clone()).collect()
}

/// `n` notices older than every one listed, keys `old<i>`.
fn older(s: &Scripted, n: usize) -> Vec<slakio_core::backend::Notice> {
    let last = s.d.app.model.notices.get(s.d.app.model.notices.len() - 1).unwrap().1.clone();
    (0..n as u64)
        .map(|i| slakio_core::backend::Notice {
            key: format!("old{i}"),
            at: Ts(last.at.0 - 1_000_000 * (i + 1)),
            ts: Ts(last.ts.0 - 1_000_000 * (i + 1)),
            ..last.clone()
        })
        .collect()
}

#[test]
fn a_page_of_notices_held_already_asks_for_no_more_by_itself() {
    let mut s = shown(120, 40, Lang::En);
    let held = listed(&s);
    let page: Vec<_> =
        (0..s.d.app.model.notices.len()).map(|i| s.d.app.model.notices.get(i).unwrap().clone()).collect();
    s.keys("G");
    assert!(!asked(&s).is_empty());
    answer_with(&mut s, |f| {
        f.notices = page.iter().filter(|(w, _)| *w == f.workspace).map(|(_, n)| n.clone()).collect();
        f.next = Some("c40".into());
    });
    assert_eq!(listed(&s), held, "nothing new");
    assert!(asked(&s).is_empty(), "a page that listed nothing new reads no further by itself");
}

#[test]
fn a_cursor_slack_hands_back_unchanged_ends_the_list() {
    let mut s = shown(120, 40, Lang::En);
    s.keys("G");
    let sent = asked(&s);
    assert!(!sent.is_empty());
    let new = older(&s, 1);
    answer_with(&mut s, |f| {
        f.notices = new
            .iter()
            .map(|n| slakio_core::backend::Notice { key: format!("{}-{}", n.key, f.workspace), ..n.clone() })
            .collect();
        // The cursor it was asked from.
        f.next = Some("c20".into());
    });
    assert!(asked(&s).is_empty(), "not the same page again by itself");
    s.keys("k j G");
    assert!(asked(&s).is_empty(), "nor on a key: the list ends there");
}

#[test]
fn at_the_most_notices_kept_no_older_page_is_asked() {
    let mut s = shown(120, 40, Lang::En);
    s.keys("G");
    let many = older(&s, slakio_tui::app::model::activity::NOTICES_MAX);
    let first = std::cell::Cell::new(true);
    answer_with(&mut s, |f| {
        if first.replace(false) {
            f.notices.clone_from(&many);
            f.next = Some("c9".into());
        }
    });
    assert_eq!(s.d.app.model.notices.len(), slakio_tui::app::model::activity::NOTICES_MAX);
    assert!(asked(&s).is_empty());
    s.keys("G");
    assert!(asked(&s).is_empty(), "an older page would be cut at once: {:?}", asked(&s));
}

/// Paged `pages` pages down (each workspace's), the cursor on the last notice.
fn paged(pages: usize) -> Scripted {
    let mut s = shown(120, 40, Lang::En);
    for _ in 0..pages {
        s.keys("G");
        answer_all(&mut s);
    }
    s.keys("G");
    answer_all(&mut s);
    s.keys("k k k");
    s
}

#[test]
fn reading_the_newest_page_again_keeps_the_older_pages_and_the_cursor() {
    let mut s = paged(3);
    let ws = s.d.app.model.workspaces()[0].id.clone();
    let before = listed(&s);
    assert!(before.len() > 60, "{} notices", before.len());
    let at = s.d.app.notice_at_cursor().unwrap().1.key.clone();
    let row = s.d.app.list_cursor() - s.d.app.list_top();
    // A mention comes live: listed at once, the cursor stays on its notice.
    let me = s.d.app.model.me(&ws).unwrap().clone();
    let c = s.d.app.model.conversations().iter().find(|c| c.workspace == ws && c.name == "deploys").unwrap().clone();
    let other =
        World::demo().snapshot().users.iter().find(|u| u.workspace == ws && u.id != me && !u.bot).unwrap().id.clone();
    let message = Message {
        ts: Ts(s.d.app.now_ts().0.max(s.d.app.model.notices.get(0).unwrap().1.at.0) + 1_000_000),
        user: other,
        text: Remote::new(format!("ping <@{me}>")),
        thread: None,
        reactions: Vec::new(),
        edited: false,
    };
    let live = Live::Message {
        workspace: ws.clone(),
        conversation: c.id.clone(),
        thread: None,
        broadcast: false,
        changed: false,
        mention: true,
        message,
        users: Vec::new(),
    };
    push(&mut s, live);
    assert_eq!(listed(&s).len(), before.len() + 1);
    assert_eq!(s.d.app.notice_at_cursor().unwrap().1.key, at, "the cursor on its notice");
    assert_eq!(s.d.app.list_cursor() - s.d.app.list_top(), row, "as far down the list");
    // The newest page read again: every older page stays, each notice once, the cursor where it was.
    s.d.now += SETTLE + FRESH;
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert_eq!(asked(&s), [(Some(ws.clone()), None)]);
    answer_all(&mut s);
    let after = listed(&s);
    assert!(before.iter().all(|k| after.contains(k)), "no older notice dropped");
    assert_eq!(after.len(), before.len() + 1, "each once, and the live mention kept until Slack lists it");
    assert!(after.iter().any(|k| k.starts_with("live:")));
    assert_eq!(s.d.app.notice_at_cursor().unwrap().1.key, at);
    assert_eq!(s.d.app.list_cursor() - s.d.app.list_top(), row);
    // Paging goes on from where the older pages end, not from the newest page's end.
    s.keys("G");
    assert!(asked(&s).iter().all(|(_, c)| c.as_deref() != Some("c20")), "{:?}", asked(&s));
}

#[test]
fn a_read_due_while_the_newest_page_is_on_its_way_does_not_spin() {
    let mut s = shown(120, 40, Lang::En);
    let ws = s.d.app.model.workspaces()[0].id.clone();
    s.keys("space h");
    s.d.now += FRESH;
    s.keys("space a");
    assert_eq!(asked(&s).len(), 2, "the newest pages, on their way");
    push(&mut s, Live::Connection { workspace: ws.clone(), state: Connection::Connected { catch_up: true } });
    s.d.now += SETTLE;
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert_eq!(asked(&s).len(), 2, "nothing more while it is on its way");
    assert!(s.d.app.deadline().is_none_or(|d| d > s.d.now), "no deadline in the past: the loop would spin");
    answer_all(&mut s);
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert!(asked(&s).is_empty(), "the read due was the one on its way");
}

#[test]
fn the_end_of_a_full_list_says_a_page_is_on_its_way_or_failed() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut s = shown(w, h, Lang::En);
        assert!(s.d.app.list_rows().len() > usize::from(h), "the list fills the panel");
        s.keys("G");
        assert!(!asked(&s).is_empty());
        let screen = s.d.screen();
        assert!(screen.contains("Loading…"), "{screen}");
        while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
            s.b.fail(i, FailKind::RateLimited { retry_after: None });
            s.pump();
        }
        let screen = s.d.screen();
        assert!(screen.contains("r retry"), "{screen}");
        // The cursor's notice still shows whole above it.
        let cursor = s.d.app.list_cursor();
        assert!(cursor + 1 < s.d.app.list_top() + usize::from(h), "{screen}");
    }
}

#[test]
fn the_list_never_starts_on_a_notices_second_row() {
    for (w, h) in [(120, 40), (80, 24), (80, 25)] {
        let mut s = shown(w, h, Lang::En);
        for keys in ["G", "j", "k k k", "G", "ctrl+d", "ctrl+u", "g g"] {
            s.keys(keys);
            answer_all(&mut s);
            let top = s.d.app.list_top();
            assert_eq!(top % 2, 0, "{w}x{h} after {keys}: top {top}");
            let rows = s.d.app.list_rows();
            assert!(matches!(rows.get(top), Some(Row::Notice(_))));
        }
    }
}

#[test]
fn t_on_a_notice_of_a_conversation_not_listed_opens_nothing() {
    let mut s = Scripted::booted(true, None);
    s.d.app.resize(120, 40);
    s.keys("space a");
    answer_with(&mut s, |f| {
        if let Some(n) = f.notices.first_mut() {
            n.conversation = slakio_core::model::ConversationId::new("CNOTLISTED1");
            n.at = Ts(n.at.0 + 1_000_000_000);
        }
    });
    let row = (s.d.app.list_rows().iter())
        .position(|r| matches!(r, Row::Notice(i) if s.d.app.model.notices.get(*i).unwrap().1.conversation.as_str() == "CNOTLISTED1"))
        .unwrap();
    s.d.app.focus_list_row(row);
    let tabs = s.d.app.tab_titles().len();
    s.keys("t");
    assert_eq!(s.d.app.tab_titles().len(), tabs, "as Enter: only a conversation the workspace lists");
    assert!(!s.b.held.iter().any(|r| !matches!(r.command, Command::Activity { .. })), "nothing asked of it");
}

#[test]
fn activity_widens_the_list_so_places_read_whole_and_l_peek_is_offered() {
    let s = shown(120, 40, Lang::En);
    assert_eq!(s.d.app.areas().list.map(|l| l.width), Some(48));
    let screen = s.d.screen();
    assert!(screen.contains("#side-project ") && !screen.contains("#side-proje…"), "{screen}");
    assert!(s.d.status_line().contains("l peek"), "{}", s.d.status_line());
}

#[test]
fn with_no_color_each_notice_says_its_workspace() {
    let mut s = shown(80, 24, Lang::En);
    s.d.app.theme = slakio_tui::theme::Theme::no_color();
    let screen = s.d.screen();
    let starts: std::collections::HashSet<char> = (screen.lines())
        .filter_map(|l| l.strip_prefix('│'))
        .filter(|l| l.chars().nth(1).is_some_and(|c| "@!&*⤷+»".contains(c)))
        .filter_map(|l| l.chars().next())
        .collect();
    assert!(starts.contains(&'A') && starts.contains(&'B'), "{starts:?}\n{screen}");
}

#[test]
fn a_read_due_for_a_workspace_signed_out_since_is_not_asked() {
    let mut s = shown(120, 40, Lang::En);
    let b = s.d.app.model.workspaces()[1].id.clone();
    s.keys("space h");
    s.d.now += FRESH;
    s.keys("space a");
    while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
        if s.b.held[i].workspace.as_ref() == Some(&b) {
            s.b.fail(i, FailKind::Auth);
            s.pump();
        } else {
            s.b.answer(i);
            s.pump();
        }
    }
    push(&mut s, Live::Connection { workspace: b.clone(), state: Connection::Connected { catch_up: true } });
    s.d.now += SETTLE + FRESH;
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert!(!asked(&s).iter().any(|(w, _)| w.as_ref() == Some(&b)), "signed out: {:?}", asked(&s));
    assert!(s.d.app.deadline().is_none_or(|d| d > s.d.now));
}

#[test]
fn at_the_top_of_the_list_a_live_mention_shows_above_the_cursor() {
    let mut s = shown(120, 40, Lang::En);
    let ws = s.d.app.model.workspaces()[0].id.clone();
    let at = s.d.app.notice_at_cursor().unwrap().1.key.clone();
    let me = s.d.app.model.me(&ws).unwrap().clone();
    let c = s.d.app.model.conversations().iter().find(|c| c.workspace == ws && c.name == "deploys").unwrap().clone();
    let other =
        World::demo().snapshot().users.iter().find(|u| u.workspace == ws && u.id != me && !u.bot).unwrap().id.clone();
    let message = Message {
        ts: Ts(s.d.app.model.notices.get(0).unwrap().1.at.0 + 1_000_000),
        user: other,
        text: Remote::new(format!("top ping <@{me}>")),
        thread: None,
        reactions: Vec::new(),
        edited: false,
    };
    push(
        &mut s,
        Live::Message {
            workspace: ws,
            conversation: c.id,
            thread: None,
            broadcast: false,
            changed: false,
            mention: true,
            message,
            users: Vec::new(),
        },
    );
    assert_eq!(s.d.app.list_top(), 0, "the top shows what is new");
    assert_eq!(s.d.app.notice_at_cursor().unwrap().1.key, at, "the cursor on its notice");
    assert!(s.d.screen().contains("top ping"));
}
