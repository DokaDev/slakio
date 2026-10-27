//! To-dos and Home's Saved, end to end over the demo backend: the view and its alias, its
//! groups, filters and orders, the changes `x` / `m` / `d` ask the backend for (nothing changes
//! before it answers; an older answer is dropped), `Enter` on an item, and the mouse.

#[path = "support/demo.rs"]
mod demo;

use demo::{Demo, assert_harmless, mask_hangul};
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use slakio_core::backend::{Envelope, Event as BackendEvent};
use slakio_core::model::{SavedState, Target};
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::View;
use slakio_tui::app::todos::Group;
use slakio_tui::app::{Focus, Mode};

/// The saved items the list shows, in its order: (text, state).
fn items(d: &Demo) -> Vec<(String, SavedState)> {
    d.app
        .list_rows()
        .iter()
        .filter_map(|r| match r {
            Row::Item(i) => {
                let s = d.app.model.saved(*i);
                Some((s.text.unsanitized().to_string(), s.state))
            }
            _ => None,
        })
        .collect()
}

/// To-dos shown, the cursor on its first item.
fn todos() -> Demo {
    let mut d = Demo::new(120, 40);
    d.keys("space o");
    d
}

#[test]
fn todos_lists_its_filter_row_then_in_progress_then_completed() {
    let mut d = todos();
    assert_eq!(d.app.view(), View::Todos);
    let rows = d.app.list_rows();
    assert_eq!(rows[0], Row::Filter);
    let groups: Vec<Group> = rows.iter().filter_map(|r| if let Row::Group(g) = r { Some(*g) } else { None }).collect();
    assert_eq!(groups, [Group::InProgress, Group::Completed]);
    // Each item is two rows: its text, then who wrote it where.
    for (k, r) in rows.iter().enumerate() {
        if let Row::Item(i) = r {
            assert_eq!(rows[k + 1], Row::ItemMeta(*i));
        }
    }
    let states: Vec<SavedState> = items(&d).into_iter().map(|(_, s)| s).collect();
    assert_eq!(states.iter().filter(|s| **s == SavedState::InProgress).count(), 6, "A company's to-dos");
    assert_eq!(states.iter().filter(|s| **s == SavedState::Completed).count(), 2);
    assert!(states.iter().all(|s| *s != SavedState::Saved), "saved messages are Home's");
    assert!(matches!(rows[d.app.list_cursor()], Row::Item(_)), "the cursor starts on the first item");
    let s = d.screen();
    assert!(s.contains("All ▾ · Due date ▾") && s.contains("In progress") && s.contains("Overdue"), "{s}");
    assert!(s.contains("│    #incidents · "), "where and when, the author's chip above: {s}");
    let mut wide = Demo::new(200, 50);
    wide.keys("space o");
    assert!(wide.screen().contains("Leo Kang in #general · "), "who too, where it fits: {}", wide.screen());
    assert_harmless(&s);
    insta::assert_snapshot!("todos_120x40", mask_hangul(&s));
    d.keys("space h space l");
    assert_eq!(d.app.view(), View::Todos, "Later's key still works");
    d.keys("space h");
    d.command("later");
    assert_eq!(d.app.view(), View::Todos, ":later is an alias");
}

#[test]
fn todos_fits_an_80x24_terminal() {
    let mut d = Demo::new(80, 24);
    d.keys("space o");
    let s = d.screen();
    assert!(s.contains("Overdue"), "{s}");
    insta::assert_snapshot!("todos_80x24", mask_hangul(&s));
}

#[test]
fn the_view_counts_to_dos_in_progress_and_overdue_is_a_warning_not_a_mention() {
    let d = todos();
    let u = d.app.view_unread(0, View::Todos);
    assert_eq!((u.todos, u.overdue, u.red()), (6, true, false));
    assert_eq!(u.badge().as_deref(), Some("6"));
}

#[test]
fn the_default_order_is_overdue_first_then_upcoming_then_none() {
    let d = todos();
    let rows = d.app.list_rows();
    let due: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Item(i) if d.app.model.saved(*i).state == SavedState::InProgress => Some(d.app.model.saved(*i).due),
            _ => None,
        })
        .collect();
    let mut want = due.clone();
    want.sort_by_key(|d| (d.is_none(), *d));
    assert_eq!(due, want);
    assert!(due[0].is_some() && due.last().unwrap().is_none());
}

#[test]
fn x_completes_a_to_do_once_the_backend_answers_and_again_reopens_it() {
    let mut d = todos();
    let (text, _) = items(&d)[0].clone();
    d.keys("x");
    assert!(d.status_line().contains("Completed"), "{}", d.status_line());
    assert!(items(&d).contains(&(text.clone(), SavedState::Completed)));
    let at =
        d.app.list_rows().iter().position(|r| matches!(r, Row::Item(i) if d.app.model.saved(*i).text == text.as_str()));
    d.app.focus_list_row(at.unwrap());
    d.keys("x");
    assert!(items(&d).contains(&(text, SavedState::InProgress)), "reopened");
    assert!(d.status_line().contains("In progress again"), "{}", d.status_line());
}

#[test]
fn a_change_waits_for_the_backend_and_an_older_answer_is_dropped() {
    let mut d = todos();
    let before = items(&d);
    // The key alone: the request is queued, nothing changes on screen.
    for k in slakio_tui::keymap::parse_keys("x").unwrap() {
        d.app.handle_event(ratatui::crossterm::event::Event::Key(k.to_event()), d.now);
    }
    assert_eq!(items(&d), before, "sent is not succeeded");
    let asked = d.app.take_commands();
    assert_eq!(asked.len(), 1);
    let stale =
        Envelope { generation: slakio_core::backend::Generation(asked[0].0.0 - 1), event: BackendEvent::Saved(vec![]) };
    assert!(!d.app.on_backend(stale), "an answer to an older request changes nothing");
    assert_eq!(items(&d), before);
}

#[test]
fn d_asks_first_and_clears_only_on_yes() {
    let mut d = todos();
    let n = items(&d).len();
    d.keys("d");
    assert!(d.screen().contains("Clear this item?"), "{}", d.screen());
    d.keys("n");
    assert_eq!(items(&d).len(), n, "kept");
    let (text, _) = items(&d)[0].clone();
    d.keys("d y");
    assert_eq!(items(&d).len(), n - 1);
    assert!(!items(&d).iter().any(|(t, _)| *t == text));
    assert!(d.status_line().contains("Cleared"), "{}", d.status_line());
}

#[test]
fn m_moves_a_to_do_to_homes_saved_and_back() {
    let mut d = todos();
    let (text, _) = items(&d)[0].clone();
    d.keys("m");
    assert!(!items(&d).iter().any(|(t, _)| *t == text), "out of To-dos");
    assert!(d.status_line().contains("Moved to Saved"), "{}", d.status_line());
    d.keys("space h");
    let rows = d.app.list_rows();
    let saved = rows.iter().position(|r| *r == Row::Group(Group::Saved)).expect("Home ends with Saved");
    assert!(rows[saved + 1..].iter().all(|r| matches!(r, Row::Item(_))), "only saved messages after it");
    let at = rows.iter().position(|r| matches!(r, Row::Item(i) if d.app.model.saved(*i).text == text.as_str()));
    let at = at.expect("the moved to-do is in Saved");
    assert_eq!(
        d.app
            .model
            .saved(match rows[at] {
                Row::Item(i) => i,
                _ => unreachable!(),
            })
            .due,
        None,
        "no reminder"
    );
    d.app.focus_list_row(at);
    d.keys("x");
    assert!(d.status_line().contains("not a to-do"), "{}", d.status_line());
    d.keys("m space o");
    assert!(items(&d).contains(&(text, SavedState::InProgress)), "back in To-dos");
}

#[test]
fn homes_saved_section_is_folded_like_a_section() {
    let mut d = Demo::new(120, 40);
    d.app.show_unfolded(View::Home);
    let rows = d.app.list_rows();
    let saved = rows.iter().position(|r| *r == Row::Group(Group::Saved)).unwrap();
    assert_eq!(rows.len() - saved - 1, 3, "A company's saved messages");
    d.app.focus_list_row(saved);
    d.keys("enter");
    assert_eq!(d.app.list_rows().len(), saved + 1, "folded");
    d.keys("l");
    assert_eq!(d.app.list_rows().len(), rows.len(), "unfolded");
}

#[test]
fn keys_of_saved_items_on_a_conversation_say_so() {
    let mut d = Demo::new(120, 40);
    d.list_cursor_on("backend");
    d.keys("x");
    assert!(d.status_line().contains("Not a saved message"), "{}", d.status_line());
}

#[test]
fn filters_narrow_to_dos_by_state_and_by_reminder() {
    let mut d = todos();
    d.command("filter overdue");
    assert!(items(&d).iter().all(|(_, s)| *s == SavedState::InProgress));
    let overdue = items(&d).len();
    assert_eq!(overdue, 2, "{:?}", items(&d));
    assert!(d.screen().contains("Overdue ▾"), "{}", d.screen());
    d.command("filter completed");
    let rows = d.app.list_rows();
    assert!(!rows.contains(&Row::Group(Group::InProgress)), "only Completed: {rows:?}");
    d.command("filter all");
    d.command("filter no-reminder");
    assert!(items(&d).iter().all(|(t, _)| {
        let rows = d.app.list_rows();
        rows.iter().any(|r| matches!(r, Row::Item(i) if d.app.model.saved(*i).text == t.as_str() && d.app.model.saved(*i).due.is_none()))
    }));
    d.command("filter all");
    assert_eq!(items(&d).len(), 8);
    d.command("filter soon");
    assert!(d.app.palette_error(d.app.size).join(" ").contains("Unknown filter soon"));
}

#[test]
fn f_and_s_open_the_palette_on_the_values() {
    let mut d = todos();
    d.keys("f");
    assert_eq!(d.app.mode(), Mode::Command);
    assert_eq!(d.app.cmdline.text(), "filter ");
    let names: Vec<String> = d.app.palette_rows().into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["all", "in-progress", "completed", "overdue", "upcoming", "no-reminder"]);
    insta::assert_snapshot!("todos_filter_palette_80x24", {
        let mut small = Demo::new(80, 24);
        small.keys("space o f");
        small.snap()
    });
    d.keys("esc s");
    assert_eq!(d.app.cmdline.text(), "sort ");
    d.type_text("new");
    d.keys("enter");
    let created: Vec<_> = d
        .app
        .list_rows()
        .iter()
        .filter_map(|r| match r {
            Row::Item(i) if d.app.model.saved(*i).state == SavedState::InProgress => {
                Some(d.app.model.saved(*i).created)
            }
            _ => None,
        })
        .collect();
    assert!(created.windows(2).all(|w| w[0] >= w[1]), "newest first");
    assert!(d.screen().contains("Newest first ▾"));
}

#[test]
fn enter_on_a_to_do_opens_its_message_selected() {
    let mut d = todos();
    let rows = d.app.list_rows();
    let Row::Item(i) = rows[d.app.list_cursor()] else { panic!() };
    let s = d.app.model.saved(i).clone();
    d.keys("enter");
    let p = d.app.focused_pane().expect("opened with the keyboard");
    let want = if s.thread {
        Target::Thread { workspace: s.workspace.clone(), conversation: s.conversation.clone(), thread: s.ts }
    } else {
        Target::Conversation { workspace: s.workspace.clone(), conversation: s.conversation.clone() }
    };
    assert_eq!(p.target(), &want);
    let at = p.selected().expect("the message is selected");
    assert_eq!(p.messages()[at].ts, s.ts);
}

#[test]
fn the_mouse_opens_an_item_from_either_of_its_rows_and_the_filter_row_parts() {
    let mut d = todos();
    let parts = d.app.list_parts().unwrap();
    let rows = d.app.list_rows();
    let meta = rows.iter().position(|r| matches!(r, Row::ItemMeta(_))).unwrap();
    let y = parts.rows.y + (meta - d.app.list_top()) as u16;
    d.mouse(MouseEventKind::Down(MouseButton::Left), parts.rows.x + 4, y);
    assert_eq!(d.app.list_cursor(), meta - 1, "its item's");
    assert!(matches!(d.app.focus(), Focus::Pane(_)), "opened");
    let filter_y = parts.rows.y;
    let sort_x = parts.rows.x + d.app.filter_row().sort_x;
    d.mouse(MouseEventKind::Down(MouseButton::Left), sort_x, filter_y);
    assert_eq!(d.app.cmdline.text(), "sort ");
    d.keys("esc");
    d.mouse(MouseEventKind::Down(MouseButton::Left), parts.rows.x + 1, filter_y);
    assert_eq!(d.app.cmdline.text(), "filter ");
}

#[test]
fn a_change_counts_only_in_its_workspace() {
    let mut d = todos();
    let b_before = d.app.view_unread(1, View::Todos).todos;
    d.keys("x");
    assert_eq!(d.app.view_unread(0, View::Todos).todos, 5);
    assert_eq!(d.app.view_unread(1, View::Todos).todos, b_before, "B side is apart");
}
