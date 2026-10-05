//! The app driven headless: key events in, frames out (Ratatui's `TestBackend`). Screens are
//! insta snapshots, English only; under CI insta never rewrites them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use slakio_core::i18n::Lang;
use slakio_tui::app::{App, Mode};
use slakio_tui::theme::Theme;
use slakio_tui::ui;
use std::time::{Duration, Instant};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn type_text(app: &mut App, text: &str, now: Instant) {
    for c in text.chars() {
        app.handle_event(key(KeyCode::Char(c)), now);
    }
}

fn screen(app: &App, w: u16, h: u16, now: Instant) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app, now)).unwrap();
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn colon_qa_enter_quits() {
    let now = Instant::now();
    let mut app = App::new(Lang::En, Theme::terminal());
    type_text(&mut app, ":qa", now);
    assert_eq!(app.mode(), Mode::Command);
    assert!(!app.quit, "not before Enter");
    app.handle_event(key(KeyCode::Enter), now);
    assert!(app.quit);
}

#[test]
fn esc_and_backspace_close_the_command_line_without_running_it() {
    let now = Instant::now();
    let mut app = App::new(Lang::En, Theme::terminal());
    type_text(&mut app, ":qa", now);
    app.handle_event(key(KeyCode::Esc), now);
    assert_eq!((app.mode(), app.quit), (Mode::Normal, false));
    type_text(&mut app, ":q", now);
    app.handle_event(key(KeyCode::Backspace), now);
    assert_eq!(app.cmdline.text(), "");
    app.handle_event(key(KeyCode::Backspace), now);
    assert_eq!((app.mode(), app.quit), (Mode::Normal, false), "Backspace on an empty line closes it");
    // An empty command does nothing.
    type_text(&mut app, ":", now);
    app.handle_event(key(KeyCode::Enter), now);
    assert_eq!((app.mode(), app.quit), (Mode::Normal, false));
}

#[test]
fn keys_outside_the_command_line_do_not_quit() {
    let now = Instant::now();
    let mut app = App::new(Lang::En, Theme::terminal());
    for c in ['q', 'a', 'Z'] {
        app.handle_event(key(KeyCode::Char(c)), now);
    }
    app.handle_event(key(KeyCode::Enter), now);
    assert!(!app.quit);
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn a_pasted_command_is_typed_without_its_line_break() {
    let now = Instant::now();
    let mut app = App::new(Lang::En, Theme::terminal());
    type_text(&mut app, ":", now);
    app.handle_event(Event::Paste("qa\n".into()), now);
    assert_eq!(app.cmdline.text(), "qa");
    assert!(!app.quit, "a paste never runs anything");
}

#[test]
fn the_empty_frame_shows_the_status_line_and_how_to_quit() {
    // How to quit and get help: keys, not a command to type.
    let now = Instant::now();
    let app = App::new(Lang::En, Theme::terminal());
    insta::assert_snapshot!("empty_80x24", screen(&app, 80, 24, now));
}

#[test]
fn an_unknown_command_warns_in_the_status_line_until_its_time_is_up() {
    let now = Instant::now();
    let mut app = App::new(Lang::En, Theme::terminal());
    type_text(&mut app, ":wq", now);
    insta::assert_snapshot!("cmdline_80x24", screen(&app, 80, 24, now));
    app.handle_event(key(KeyCode::Enter), now);
    assert!(!app.quit);
    let shown = screen(&app, 80, 24, now);
    assert!(shown.lines().last().unwrap().contains("Not a command: wq"), "{shown}");
    let at = app.deadline().expect("the notice goes away by itself");
    assert!(at > now && at <= now + Duration::from_secs(10));
    assert!(app.on_tick(at), "the screen changes when it goes");
    assert!(!screen(&app, 80, 24, at).contains("Not a command"));
    assert_eq!(app.deadline(), None, "then nothing to wake up for");
}

#[test]
fn a_small_terminal_still_draws_without_panicking() {
    let now = Instant::now();
    let mut app = App::new(Lang::En, Theme::terminal());
    for (w, h) in [(1, 1), (10, 2), (20, 1), (0, 0)] {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| ui::draw(f, &app, now)).unwrap();
        type_text(&mut app, ":qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq", now);
        t.draw(|f| ui::draw(f, &app, now)).unwrap();
        app.handle_event(key(KeyCode::Esc), now);
    }
}

#[test]
fn korean_ui_shows_the_translated_hint() {
    let now = Instant::now();
    let ko = App::new(Lang::Ko, Theme::terminal());
    let en = App::new(Lang::En, Theme::terminal());
    let (k, e) = (screen(&ko, 80, 24, now), screen(&en, 80, 24, now));
    assert!(!k.contains("Keyboard help"), "{k}");
    assert!(e.contains("Keyboard help") && e.contains("Ctrl+Q"), "{e}");
    assert!(k.contains("Ctrl+Q"), "the keys stay keys: {k}");
}
