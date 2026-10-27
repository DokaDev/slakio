//! The sign-in screen driven headless: what it says, a paste handed to the binary (never drawn,
//! never printed), one paste at a time, and connecting once signed in.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::Event;
use slakio_core::backend::Capabilities;
use slakio_core::i18n::{Label, Lang};
use slakio_tui::app::signin::{Pasted, SignIn};
use slakio_tui::app::{App, Effect};
use slakio_tui::theme::Theme;
use slakio_tui::ui;
use std::time::Instant;

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
    app.signed_in(Capabilities { demo: false, read_only: true }, "Example".into(), now);
    assert_eq!(app.sign_in(), None);
    assert!(app.read_only());
    assert_eq!(app.take_commands().len(), 1, "it asks for the workspace");
    assert!(screen(&app, 100, 30, now).contains("Signed in to Example"));
}

#[test]
fn a_paste_is_wiped_when_dropped() {
    fn wiped_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    wiped_on_drop::<Pasted>();
}
