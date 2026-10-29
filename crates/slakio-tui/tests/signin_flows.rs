//! The sign-in screen driven headless: what it says, a paste handed to the binary (never drawn,
//! never printed), one paste at a time, and connecting once signed in.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use slakio_core::backend::Capabilities;
use slakio_core::i18n::{Label, Lang};
use slakio_core::model::WorkspaceId;
use slakio_tui::app::model::SignedIn;
use slakio_tui::app::signin::{Pasted, Picker, PickerRow, SignIn};
use slakio_tui::app::{App, Effect};
use slakio_tui::theme::Theme;
use slakio_tui::ui;
use std::time::Instant;

/// Send one key press to the app.
fn key(app: &mut App, code: KeyCode) {
    app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)), Instant::now());
}

fn picker() -> App {
    let mut app = App::new(Lang::En, Theme::terminal());
    let row =
        |team: &str, name: &str, host: &str| PickerRow { team: team.into(), name: name.into(), host: host.into() };
    app.offer_desktop(vec![
        row("T1", "Acme Corp", "acme.slack.com"),
        row("T2", "Acme Corp", "acme-eu.slack.com"),
        row("T3", "Gamma", "gamma.slack.com"),
    ]);
    app
}

/// A synthetic secret-shaped string (spelled in parts so no file holds a token-shaped one).
const SECRET: &str = concat!("xo", "xc-1-2-SECRETVALUE");

fn screen(app: &App, w: u16, h: u16, now: Instant) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app, now)).unwrap();
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn signing_in() -> App {
    let mut app = App::new(Lang::En, Theme::terminal());
    app.offer_sign_in(None);
    app
}

#[test]
fn the_screen_says_how_to_copy_a_request() {
    let now = Instant::now();
    let app = signing_in();
    insta::assert_snapshot!("signin_100x30", screen(&app, 100, 30, now));
}

#[test]
fn a_paste_goes_to_the_binary_once_and_is_never_drawn() {
    let now = Instant::now();
    let mut app = signing_in();
    let text = format!("curl 'https://example.slack.com/api/x' --data-raw 'token={SECRET}'");
    app.handle_event(Event::Paste(text.clone()), now);
    assert_eq!(app.sign_in(), Some(SignIn::Checking));
    let effects = app.take_effects();
    assert_eq!(effects, vec![Effect::SignIn(Pasted(text.clone()))]);
    assert!(!format!("{effects:?}").contains(SECRET), "a paste never prints");
    // A second paste while checking is not taken.
    app.handle_event(Event::Paste(text), now);
    assert!(app.take_effects().is_empty());
    let shown = screen(&app, 100, 30, now);
    assert!(!shown.contains("SECRETVALUE") && shown.contains("Checking the sign-in"), "{shown}");
    assert_eq!(app.cmdline.text(), "", "nothing was typed anywhere");
}

#[test]
fn a_failed_sign_in_says_why_and_takes_another_paste() {
    let now = Instant::now();
    let mut app = signing_in();
    app.handle_event(Event::Paste("x".into()), now);
    app.sign_in_failed(Label::SigninNoCookie);
    insta::assert_snapshot!("signin_failed_100x30", screen(&app, 100, 30, now));
    app.handle_event(Event::Paste("y".into()), now);
    assert_eq!(app.take_effects().len(), 2, "the first paste and this one");
}

#[test]
fn a_keychain_that_could_not_be_read_is_said_on_the_screen() {
    let now = Instant::now();
    let mut app = App::new(Lang::En, Theme::terminal());
    app.offer_sign_in(Some(Label::SigninStoreRead));
    assert!(screen(&app, 100, 30, now).contains("Could not read the sign-in from the keychain"));
}

#[test]
fn once_signed_in_the_app_connects_and_says_where() {
    let now = Instant::now();
    let mut app = signing_in();
    app.handle_event(Event::Paste("x".into()), now);
    let signed = [
        SignedIn { id: WorkspaceId::new("T1"), name: "Kept".into(), read_only: true, user: String::new() },
        SignedIn { id: WorkspaceId::new("T2"), name: "Example".into(), read_only: true, user: String::new() },
    ];
    app.signed_in(Capabilities { demo: false, read_only: false }, &signed, None, &["Example".into()], now);
    assert_eq!(app.sign_in(), None);
    assert!(app.read_only(), "each workspace is read-only");
    let asked = app.take_commands(std::time::Instant::now());
    let boots: Vec<_> = asked.iter().map(|r| r.workspace.clone()).collect();
    assert_eq!(boots, vec![Some(signed[0].id.clone()), Some(signed[1].id.clone())], "each workspace is asked for");
    assert!(screen(&app, 100, 30, now).contains("Signed in to Example"));
    assert_eq!(app.model.workspaces().len(), 2, "the one signed in to before stays listed");
}

#[test]
fn signing_in_beside_a_workspace_says_it_adds_one_and_lands_on_the_one_added() {
    let now = Instant::now();
    let mut app = App::new(Lang::En, Theme::terminal());
    app.sign_in_beside(vec!["A company".into()]);
    app.offer_sign_in(None);
    let said = "Signed in to A company already: this adds another workspace";
    for (w, h) in [(120, 40), (80, 24)] {
        assert!(screen(&app, w, h, now).contains(said), "{w}x{h}: {}", screen(&app, w, h, now));
    }
    app.offer_desktop(vec![PickerRow { team: "T2".into(), name: "B side".into(), host: "b.slack.com".into() }]);
    assert!(screen(&app, 120, 40, now).contains(said), "the picker says it too");
    // Nothing is said at the first sign-in.
    let mut first = App::new(Lang::En, Theme::terminal());
    first.offer_sign_in(None);
    assert!(!screen(&first, 120, 40, now).contains("already"));
    // Signed in: the one just added is shown, the first stays first in the switcher.
    let signed = [
        SignedIn { id: WorkspaceId::new("T1"), name: "A company".into(), read_only: true, user: String::new() },
        SignedIn { id: WorkspaceId::new("T2"), name: "B side".into(), read_only: true, user: String::new() },
    ];
    app.signed_in(Capabilities { demo: false, read_only: true }, &signed, Some(&signed[1].id), &["B side".into()], now);
    assert_eq!(app.workspace(), 1, "B, just added, is shown");
    assert_eq!(app.model.workspaces()[0].id, signed[0].id);
    assert!(app.signed_in_already().is_empty(), "signing in is done");
}

#[test]
fn a_paste_is_wiped_when_dropped() {
    fn wiped_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    wiped_on_drop::<Pasted>();
}

#[test]
fn the_screen_waits_for_the_paste_and_says_how() {
    let now = Instant::now();
    let app = signing_in();
    for (w, h) in [(80, 24), (100, 30), (60, 30)] {
        let shown = screen(&app, w, h, now);
        assert!(shown.contains("Waiting for the paste"), "{shown}");
        assert!(shown.lines().last().unwrap().contains("Cmd+V paste"), "{shown}");
        assert!(shown.contains("Copy > Copy as cURL"), "the menu path stays on one line at {w}: {shown}");
        // A wrapped step goes on under its text, not under its number.
        let lines: Vec<&str> = shown.lines().collect();
        let step = lines.iter().position(|l| l.trim_start().starts_with("2. ")).unwrap();
        let at = lines[step].find("2. ").unwrap();
        let next = lines[step + 1];
        if !next.trim_start().starts_with("3. ") {
            assert_eq!(next.find(|c: char| c != ' '), Some(at + 3), "hanging indent at {w}: {shown}");
        }
    }
}

#[test]
fn the_desktop_picker_lists_the_workspaces_with_their_hosts() {
    let now = Instant::now();
    let app = picker();
    let shown = screen(&app, 100, 30, now);
    assert!(shown.contains("Sign in from the Slack desktop app"), "{shown}");
    // Two workspaces of the same name are told apart by their hosts.
    let rows: Vec<&str> = shown.lines().filter(|l| l.contains("Acme Corp")).collect();
    assert_eq!(rows.len(), 2, "{shown}");
    assert!(
        rows[0].trim_end().ends_with("acme.slack.com") && rows[1].trim_end().ends_with("acme-eu.slack.com"),
        "{shown}"
    );
    assert!(!shown.contains("> "), "no ASCII cursor: the selection bar marks the row: {shown}");
    let flat = shown.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("The prompt names \"security\""), "the requester the prompt shows: {shown}");
    insta::assert_snapshot!("signin_desktop_100x30", shown);
}

#[test]
fn the_selected_workspace_has_the_selection_bar_and_a_bold_name() {
    let app = picker();
    let buf = buffer(&app, 100, 30);
    let (x, y) = find(&buf, "Acme Corp").expect("the first row");
    let theme = Theme::terminal();
    let mut bar = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 1, 1));
    theme.paint_selection(&mut bar, ratatui::layout::Rect::new(0, 0, 1, 1), slakio_tui::theme::Selection::Focused);
    assert_eq!(buf[(x, y)].bg, bar[(0, 0)].bg, "the selection bar");
    assert!(buf[(x, y)].modifier.contains(ratatui::style::Modifier::BOLD), "a bold name");
    let (_, other) = find_from(&buf, "Acme Corp", y + 1).expect("the second row");
    assert!(!buf[(x, other)].modifier.contains(ratatui::style::Modifier::BOLD), "only the selected one");
}

#[test]
fn the_allow_advice_is_one_emphasised_line_in_every_state() {
    let advice = "press Allow, not Always Allow.";
    let mut app = picker();
    let check = |app: &App, state: &str| {
        let buf = buffer(app, 100, 30);
        let (x, y) = find(&buf, advice).unwrap_or_else(|| panic!("{state}: {}", screen(app, 100, 30, Instant::now())));
        assert_eq!(buf[(x, y)].fg, Theme::terminal().warning().fg.unwrap(), "{state}: warning-coloured");
        let line = row(&buf, y);
        assert!(
            line.trim_start().starts_with("When macOS asks about \"Slack Safe Storage\""),
            "{state}: one line: {line}"
        );
    };
    check(&app, "choosing");
    key(&mut app, KeyCode::Enter);
    check(&app, "checking");
    assert!(screen(&app, 100, 30, Instant::now()).contains("Reading the Slack app's key"));
    app.sign_in_failed(Label::SigninDesktopKeychain);
    check(&app, "after a failure");
}

#[test]
fn a_paste_on_the_picker_is_taken_as_a_sign_in() {
    let now = Instant::now();
    let mut app = picker();
    let text = format!("curl 'https://example.slack.com/api/x' --data-raw 'token={SECRET}'");
    app.handle_event(Event::Paste(text.clone()), now);
    assert_eq!(app.sign_in(), Some(SignIn::Checking));
    assert_eq!(app.take_effects(), vec![Effect::SignIn(Pasted(text))]);
}

#[test]
fn the_cursor_moves_and_enter_picks_the_selected_workspace() {
    let mut app = picker();
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Down);
    assert!(matches!(app.sign_in(), Some(SignIn::Desktop(Picker { selected: 2, .. }))));
    key(&mut app, KeyCode::Down);
    assert!(matches!(app.sign_in(), Some(SignIn::Desktop(Picker { selected: 2, .. }))), "stops at the last");
    key(&mut app, KeyCode::Char('k'));
    assert!(matches!(app.sign_in(), Some(SignIn::Desktop(Picker { selected: 1, .. }))));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.take_effects(), vec![Effect::DesktopPick(vec!["T2".into()])], "picked by team id");
    assert!(matches!(app.sign_in(), Some(SignIn::DesktopChecking(_))));
}

#[test]
fn p_falls_back_to_the_paste_flow() {
    let mut app = picker();
    key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.sign_in(), Some(SignIn::Waiting));
    assert!(app.take_effects().is_empty());
    assert!(screen(&app, 100, 30, Instant::now()).contains("Copy > Copy as cURL"));
}

#[test]
fn a_failed_pick_returns_to_the_picker_with_the_reason_and_the_keys() {
    let mut app = picker();
    key(&mut app, KeyCode::Enter);
    let _ = app.take_effects();
    app.sign_in_failed(Label::SigninDesktopKeychain);
    match app.sign_in() {
        Some(SignIn::Desktop(p)) => {
            assert_eq!(p.selected, 0);
            assert_eq!(p.note, Some(Label::SigninDesktopKeychain));
        }
        other => panic!("expected the picker again, got {other:?}"),
    }
    let flat = screen(&app, 100, 30, Instant::now()).split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("Press Enter to try again, or p to paste a request"), "{flat}");
    assert!(!flat.contains("Run slakio login"), "already there: {flat}");
    assert!(
        flat.contains("Enter to sign in, Space to mark several, j/k or Up/Down to choose"),
        "the keys stay: {flat}"
    );
    // Enter tries again.
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.take_effects(), vec![Effect::DesktopPick(vec!["T1".into()])]);
}

#[test]
fn the_picker_says_its_keys_on_the_status_line_and_in_the_help() {
    let mut app = picker();
    let shown = screen(&app, 120, 30, Instant::now());
    let status = shown.lines().last().unwrap();
    for hint in ["Enter sign in", "p paste a request", "Cmd+V paste"] {
        assert!(status.contains(hint), "{hint}: {status}");
    }
    key(&mut app, KeyCode::Char('?'));
    let help = screen(&app, 120, 40, Instant::now());
    assert!(help.contains("Sign-in: Slack desktop app workspaces"), "{help}");
    assert!(
        help.contains("Sign in to the marked workspaces") && help.contains("Paste a browser request instead"),
        "{help}"
    );
    assert!(help.contains("Mark this workspace to sign in to it too"), "{help}");
}

#[test]
fn a_long_list_scrolls_to_keep_the_cursor_in_view() {
    let mut app = App::new(Lang::En, Theme::terminal());
    app.offer_desktop(
        (0..30)
            .map(|i| PickerRow { team: format!("T{i}"), name: format!("Team {i:02}"), host: format!("t{i}.slack.com") })
            .collect(),
    );
    for _ in 0..29 {
        key(&mut app, KeyCode::Char('j'));
    }
    let shown = screen(&app, 80, 24, Instant::now());
    assert!(shown.contains("Team 29") && !shown.contains("Team 00"), "{shown}");
}

fn buffer(app: &App, w: u16, h: u16) -> ratatui::buffer::Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app, Instant::now())).unwrap();
    t.backend().buffer().clone()
}

fn row(buf: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect()
}

/// Where `text` starts on the screen, from row `from` down.
fn find_from(buf: &ratatui::buffer::Buffer, text: &str, from: u16) -> Option<(u16, u16)> {
    (from..buf.area.height).find_map(|y| {
        let line = row(buf, y);
        line.find(text).map(|i| (line[..i].chars().count() as u16, y))
    })
}

fn find(buf: &ratatui::buffer::Buffer, text: &str) -> Option<(u16, u16)> {
    find_from(buf, text, 0)
}

#[test]
fn at_80_columns_the_picker_keeps_its_own_keys_and_drops_the_paste_hint_first() {
    let app = picker();
    let shown = screen(&app, 80, 24, Instant::now());
    let status = shown.lines().last().unwrap();
    assert!(status.contains("Enter sign in · j/k choose · p paste a request"), "{status}");
    let p = status.find("p paste a request").unwrap();
    assert!(status.find("Cmd+V").is_none_or(|c| c > p), "Cmd+V paste comes after p: {status}");
}

#[test]
fn no_paste_is_offered_while_a_sign_in_is_being_checked() {
    let mut app = picker();
    key(&mut app, KeyCode::Enter);
    assert!(matches!(app.sign_in(), Some(SignIn::DesktopChecking(_))));
    let status = screen(&app, 120, 30, Instant::now()).lines().last().unwrap().to_string();
    assert!(!status.contains("Cmd+V"), "a paste is not taken now: {status}");
    // The paste screen offers it first; while its paste is checked, not at all.
    let mut app = App::new(Lang::En, Theme::terminal());
    app.offer_sign_in(None);
    let status = screen(&app, 120, 30, Instant::now()).lines().last().unwrap().to_string();
    assert!(
        status
            .trim_start()
            .trim_start_matches("NORMAL")
            .trim_start()
            .trim_start_matches('│')
            .trim_start()
            .starts_with("Cmd+V paste"),
        "{status}"
    );
    app.handle_event(Event::Paste("curl 'https://x.slack.com/api/a'".into()), Instant::now());
    assert_eq!(app.sign_in(), Some(SignIn::Checking));
    let status = screen(&app, 120, 30, Instant::now()).lines().last().unwrap().to_string();
    assert!(!status.contains("Cmd+V"), "{status}");
}

#[test]
fn a_scrolled_picker_says_how_many_more_there_are_above_and_below() {
    let mut app = App::new(Lang::En, Theme::terminal());
    app.offer_desktop(
        (0..30)
            .map(|i| PickerRow { team: format!("T{i}"), name: format!("Team {i:02}"), host: format!("t{i}.slack.com") })
            .collect(),
    );
    let at = |app: &App| screen(app, 80, 24, Instant::now());
    let top = at(&app);
    assert!(top.contains("Team 00") && top.contains("more") && top.contains('↓'), "{top}");
    assert!(!top.contains('↑'), "nothing above the first: {top}");
    let shown = top.lines().filter(|l| l.contains("Team ")).count();
    assert!(top.contains(&format!("↓ {} more", 30 - shown)), "{top}");
    for _ in 0..15 {
        key(&mut app, KeyCode::Char('j'));
    }
    let middle = at(&app);
    assert!(middle.contains('↑') && middle.contains('↓'), "{middle}");
    for _ in 0..20 {
        key(&mut app, KeyCode::Char('j'));
    }
    let end = at(&app);
    assert!(end.contains("Team 29") && end.contains('↑') && !end.contains('↓'), "{end}");
    let shown = end.lines().filter(|l| l.contains("Team ")).count();
    assert!(end.contains(&format!("↑ {} more", 30 - shown)), "{end}");
    // The marks are muted, not text of a row.
    let buf = {
        let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
        t.draw(|f| ui::draw(f, &app, Instant::now())).unwrap();
        t.backend().buffer().clone()
    };
    let (x, y) = find_from(&buf, "↑", 0).unwrap();
    assert_eq!(buf[(x, y)].fg, app.theme.muted().fg.unwrap_or_default(), "muted");
    // A list that fits has no marks.
    let fits = at(&picker());
    assert!(!fits.contains("more") && !fits.contains('↑') && !fits.contains('↓'), "{fits}");
}

#[test]
fn a_scrolled_picker_at_80x24_keeps_its_rows_for_workspaces_and_the_marks_on_the_blank_lines() {
    let mut app = App::new(Lang::En, Theme::terminal());
    app.offer_desktop(
        (0..19)
            .map(|i| PickerRow { team: format!("T{i}"), name: format!("Team {i:02}"), host: format!("t{i}.slack.com") })
            .collect(),
    );
    for _ in 0..9 {
        key(&mut app, KeyCode::Char('j'));
    }
    let shown = screen(&app, 80, 24, Instant::now());
    let lines: Vec<&str> = shown.lines().collect();
    let names = lines.iter().filter(|l| l.contains("Team ")).count();
    assert!(names >= 5, "{names} of 19 shown: {shown}");
    let pick = lines.iter().position(|l| l.contains("signed in to these workspaces:")).unwrap();
    assert!(lines[pick + 1].contains("↑ ") && lines[pick + 1].contains("more"), "{shown}");
    assert!(lines[pick + 2].contains("Team "), "{shown}");
    let allow = lines.iter().position(|l| l.contains("Slack Safe Storage")).unwrap();
    assert!(lines[allow - 1].contains("↓ ") && lines[allow - 1].contains("more"), "{shown}");
    assert!(lines[allow - 2].contains("Team "), "{shown}");
    // At the top nothing is above: that line stays blank, the rows do not move.
    let mut top = App::new(Lang::En, Theme::terminal());
    top.offer_desktop(
        (0..19)
            .map(|i| PickerRow { team: format!("T{i}"), name: format!("Team {i:02}"), host: format!("t{i}.slack.com") })
            .collect(),
    );
    let at_top = screen(&top, 80, 24, Instant::now());
    let rows_at = |s: &str| s.lines().position(|l| l.contains("Team ")).unwrap();
    assert_eq!(rows_at(&at_top), rows_at(&shown), "{at_top}");
    assert!(!at_top.contains('↑'), "{at_top}");
}

#[test]
fn a_scrolled_picker_says_how_many_of_the_rows_out_of_view_are_marked() {
    let mut app = App::new(Lang::En, Theme::terminal());
    app.offer_desktop(
        (0..19)
            .map(|i| PickerRow { team: format!("T{i}"), name: format!("Team {i:02}"), host: format!("t{i}.slack.com") })
            .collect(),
    );
    // Team 00, 03 and 06 marked; the cursor on 09, so 05 to 09 show.
    for k in 0..9 {
        if k % 3 == 0 {
            key(&mut app, KeyCode::Char(' '));
        }
        key(&mut app, KeyCode::Char('j'));
    }
    let shown = screen(&app, 80, 24, Instant::now());
    assert!(shown.contains("✓ Team 06"), "{shown}");
    assert!(shown.contains("↑ 5 more · 2 marked"), "two of the five above are marked: {shown}");
    assert!(shown.contains("↓ 9 more") && !shown.contains("9 more ·"), "none below is: {shown}");
}

#[test]
fn space_marks_several_and_enter_signs_in_to_them_in_the_pickers_order() {
    let mut app = picker();
    // Gamma first, then the first Acme: Enter takes them in the list's order.
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Char('k'));
    key(&mut app, KeyCode::Char('k'));
    key(&mut app, KeyCode::Char(' '));
    // The second Acme marked and unmarked again.
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Char(' '));
    let Some(SignIn::Desktop(p)) = app.sign_in() else { panic!("the picker") };
    assert_eq!((p.is_marked(0), p.is_marked(1), p.is_marked(2)), (true, false, true));
    for (w, h) in [(120, 40), (80, 24)] {
        let shown = screen(&app, w, h, Instant::now());
        let marked: Vec<&str> = shown.lines().filter(|l| l.contains('✓')).collect();
        assert_eq!(marked.len(), 2, "{w}x{h}: {shown}");
        assert!(marked[0].contains("acme.slack.com") && marked[1].contains("Gamma"), "{w}x{h}: {shown}");
        insta::assert_snapshot!(format!("signin_desktop_marked_{w}x{h}"), shown);
    }
    let status = screen(&app, 120, 40, Instant::now()).lines().last().unwrap().to_string();
    assert!(status.contains("Space mark"), "{status}");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.take_effects(), vec![Effect::DesktopPick(vec!["T1".into(), "T3".into()])]);
    // While they are checked, the marks stay; a failure brings the picker back with them.
    assert!(screen(&app, 120, 40, Instant::now()).contains('✓'));
    app.sign_in_failed(Label::SigninRefused);
    let Some(SignIn::Desktop(p)) = app.sign_in() else { panic!("the picker again") };
    assert_eq!(p.marked, vec!["T3".to_string(), "T1".to_string()], "as marked, for another try");
    assert_eq!(p.chosen(), vec!["T1".to_string(), "T3".to_string()]);
}

#[test]
fn enter_with_nothing_marked_signs_in_to_the_one_under_the_cursor() {
    let mut app = picker();
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.take_effects(), vec![Effect::DesktopPick(vec!["T2".into()])]);
    // No picker rows: Enter does nothing.
    let mut empty = App::new(Lang::En, Theme::terminal());
    empty.offer_desktop(Vec::new());
    key(&mut empty, KeyCode::Enter);
    assert!(empty.take_effects().is_empty());
}
