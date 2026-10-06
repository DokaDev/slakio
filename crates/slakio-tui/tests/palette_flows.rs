//! The command palette (`Ctrl+P` or `:`): a box near the top of the dimmed screen, the input
//! first, then every command and action that works where the keyboard is, each with its keys;
//! typing filters them (commands by name, actions by their words), `Tab`/arrows select, `Enter`
//! runs, the mouse wheel and a click do the same, `Esc` or a click outside closes it.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use slakio_core::i18n::Lang;
use slakio_tui::app::dialog::Question;
use slakio_tui::app::shell::View;
use slakio_tui::app::{Focus, Mode, Overlay, Settings};
use slakio_tui::theme::{Background, resolve};

/// The palette's lines (the box) of the screen.
fn palette(d: &Demo) -> Vec<String> {
    let b = d.app.palette_box().expect("the palette is open");
    let s = d.screen();
    let lines: Vec<&str> = s.lines().collect();
    (b.rect.top()..b.rect.bottom()).map(|y| lines[usize::from(y)].to_string()).collect()
}

#[test]
fn ctrl_p_opens_a_box_of_every_command_and_action_with_its_keys() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+p");
    assert_eq!(d.app.mode(), Mode::Command);
    let b = d.app.palette_box().unwrap();
    assert_eq!((b.rect.width, b.rect.y), (72, 6), "60% of the width, a sixth down");
    assert_eq!(b.rect.x, (120 - 72) / 2, "centered");
    let lines = palette(&d);
    assert!(lines[0].contains("╭ Commands "), "{lines:#?}");
    assert!(lines[1].contains(": type a command or an action name"), "{lines:#?}");
    assert!(lines[2].contains("├────"), "a rule joined to the sides: {lines:#?}");
    let all = lines.join("\n");
    for want in [":theme <theme>", ":help", "? / F1"] {
        assert!(all.contains(want), "{want}: {all}");
    }
    // The list scrolls: the rest is there too. Quit is last, never the entry Enter would run.
    let rows = d.app.palette_rows();
    let last = rows.last().unwrap();
    assert_eq!((last.name.as_str(), last.label.as_str()), (":qa, :qall, :quitall", "Quit"), "{rows:#?}");
    assert_eq!(last.keys, "Ctrl+Q / Space q");
    assert!(!all.contains("Quit"), "not in the first screen: {all}");
    assert_ne!(rows[d.app.cmdline.selected].label, "Quit");
    for (name, keys) in [(":theme <theme>", ""), (":rail", "Ctrl+R / Space r"), (":home", "Space h")] {
        assert!(rows.iter().any(|r| r.name == name && r.keys == keys), "{name}: {rows:#?}");
    }
    assert!(rows.iter().any(|r| r.name.is_empty() && r.label == "Next panel" && r.keys == "Tab / F6"), "{rows:#?}");
    assert!(lines.last().unwrap().contains("Tab/↑↓ select · Enter run · Esc close"), "{lines:#?}");
    insta::assert_snapshot!("palette_open_120x40", d.snap());
    // The same key closes it.
    d.keys("ctrl+p");
    assert_eq!(d.app.mode(), Mode::Normal);
}

#[test]
fn colon_opens_the_same_palette_and_typing_filters_it() {
    let mut d = Demo::new(80, 24);
    d.keys(":");
    assert!(d.app.palette_box().is_some());
    d.type_text("dm");
    let rows = d.app.palette_rows();
    assert_eq!(rows[0].name, ":dms", "a command the word starts comes first: {rows:?}");
    assert!(rows.iter().skip(1).all(|r| r.name.is_empty()), "then actions found by their words: {rows:?}");
    insta::assert_snapshot!("palette_filter_80x24", d.snap());
    d.keys("enter");
    assert_eq!((d.app.view(), d.app.mode()), (View::Dms, Mode::Normal));
}

#[test]
fn actions_are_found_by_their_words_and_run_with_enter() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+p");
    d.type_text("rail");
    let rows = d.app.palette_rows();
    assert_eq!(rows[0].name, ":rail");
    assert_eq!(rows[0].keys, "Ctrl+R / Space r", "the keys from where the keyboard is");
    d.keys("enter");
    assert_eq!(d.app.focus(), Focus::Rail);
    // An action without a command: found by its label.
    d.keys("esc ctrl+p");
    d.type_text("next panel");
    assert!(d.app.palette_rows().is_empty(), "two words are a command and its argument");
    d.keys("ctrl+p ctrl+p");
    d.type_text("nextpan");
    assert_eq!(d.app.palette_rows()[0].label, "Next panel");
    d.keys("enter");
    assert_eq!(d.app.focus(), Focus::Rail, "Tab's action ran: nothing is open, the rail is next");
}

#[test]
fn tab_and_arrows_select_and_enter_on_nothing_typed_runs_only_a_picked_entry() {
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.keys("enter");
    assert_eq!(d.app.mode(), Mode::Normal, "Enter on an empty line closes, runs nothing");
    assert!(!d.app.quit);
    d.keys(": tab tab down up shift+tab ctrl+n");
    assert_eq!(d.app.cmdline.selected, 2);
    let label = d.app.palette_rows()[2].label.clone();
    d.keys("up");
    assert_eq!(d.app.cmdline.selected, 1);
    d.keys("up up");
    let n = d.app.palette_rows().len();
    assert_eq!(d.app.cmdline.selected, n - 1, "round past the top");
    assert!(!label.is_empty());
}

#[test]
fn theme_lists_the_themes_marks_the_current_one_and_switches() {
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.type_text("theme");
    assert_eq!(d.app.palette_rows()[0].name, ":theme <theme>");
    d.keys("enter");
    assert_eq!(d.app.cmdline.text(), "theme ", "Enter completes a command that takes an argument");
    let rows = d.app.palette_rows();
    assert_eq!(rows.len(), slakio_tui::theme::NAMES.len());
    assert_eq!((rows[0].name.as_str(), rows[0].keys.as_str()), ("auto", "current"));
    assert!(rows.iter().any(|r| r.name == "gruvbox" && r.label.contains("light or dark")));
    d.type_text("gru");
    let names: Vec<String> = d.app.palette_rows().into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["gruvbox", "gruvbox-light", "gruvbox-dark"]);
    d.keys("down enter");
    assert_eq!(d.app.theme.name, "gruvbox-light");
    assert_eq!(d.app.mode(), Mode::Normal);
    let mut small = Demo::new(80, 24);
    small.keys(":");
    small.type_text("theme ");
    insta::assert_snapshot!("palette_themes_80x24", small.snap());
}

#[test]
fn what_runs_nothing_says_why_in_the_palette_which_stays() {
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.type_text("wq");
    d.keys("enter");
    assert!(!d.app.quit);
    assert_eq!(d.app.mode(), Mode::Command, "the palette stays to fix the line");
    let all = palette(&d).join("\n");
    assert!(all.contains("No matching commands") && all.contains("Not a command: wq"), "{all}");
    d.keys("backspace backspace");
    d.type_text("home x");
    d.keys("enter");
    assert!(palette(&d).join("\n").contains(":home takes no argument"));
    for _ in 0..6 {
        d.keys("backspace");
    }
    d.type_text("theme solarized");
    d.keys("enter");
    let all = palette(&d).join("\n");
    assert!(all.contains("Unknown theme solarized") && all.contains("dracula"), "every name, wrapped: {all}");
    d.keys("esc");
    assert_eq!(d.app.mode(), Mode::Normal);
}

#[test]
fn the_mouse_scrolls_picks_and_closes() {
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.type_text("show");
    let b = d.app.palette_box().unwrap();
    assert!(d.mouse(MouseEventKind::ScrollDown, b.list.x + 4, b.list.y));
    assert_eq!(d.app.cmdline.selected, 1);
    let at = d.app.palette_rows().iter().position(|r| r.label == "Show DMs").expect("Show DMs");
    assert!(at < usize::from(b.list.height));
    assert!(d.mouse(MouseEventKind::Down(MouseButton::Left), b.list.x + 4, b.list.y + at as u16));
    assert_eq!((d.app.view(), d.app.mode()), (View::Dms, Mode::Normal), "a click runs the entry");
    d.keys(":");
    assert!(d.mouse(MouseEventKind::Down(MouseButton::Left), 1, 39));
    assert_eq!(d.app.mode(), Mode::Normal, "a click outside closes it");
}

#[test]
fn the_palette_is_translated_and_drawn_in_every_theme() {
    let mut d = Demo::with(120, 40, Lang::Ko, Settings::default());
    d.keys(":");
    let s = d.screen();
    assert!(!s.contains("Commands") && !s.contains("type a command"), "{s}");
    for name in slakio_tui::theme::NAMES {
        let mut d = Demo::new(120, 40);
        d.app.theme = resolve(name, true, Background::Dark);
        d.keys("ctrl+p");
        let buf = d.buffer();
        let b = d.app.palette_box().unwrap();
        // The selected entry is on the selection bar, the rest on the popup's surface.
        let sel = buf[(b.list.x + 1, b.list.y)].bg;
        let other = buf[(b.list.x + 1, b.list.y + 1)].bg;
        assert_eq!(other, d.app.theme.surface, "{name}");
        if d.app.theme.kind == slakio_tui::theme::Kind::Truecolor {
            assert_eq!(sel, d.app.theme.selection, "{name}");
        }
        // The badge stays bright under the dimmed screen.
        assert_eq!(buf[(1, 39)].bg, d.app.theme.mode_command, "{name}");
    }
}

#[test]
fn letters_in_order_find_an_entry_ranked_name_then_word_start_then_letters() {
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.type_text("thm");
    assert_eq!(d.app.palette_rows()[0].name, ":theme <theme>", "thm finds the theme command");
    d.keys("ctrl+p ctrl+p");
    d.type_text("hom");
    assert_eq!(d.app.palette_rows()[0].name, ":home", "a name it starts first");
    d.keys("ctrl+p ctrl+p");
    d.type_text("color");
    assert_eq!(d.app.palette_rows()[0].name, ":theme <theme>", "a word of the label");
    d.keys("ctrl+p ctrl+p");
    d.type_text("shdm");
    assert_eq!(d.app.palette_rows()[0].label, "Show DMs", "letters in order from a word start");
    d.keys("ctrl+p ctrl+p");
    d.type_text("theme tkn");
    assert_eq!(d.app.palette_rows()[0].name, "tokyo-night", "theme names the same way");
}

#[test]
fn a_click_on_quit_asks_first_and_qa_enter_still_quits() {
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.type_text("qa");
    let b = d.app.palette_box().unwrap();
    assert_eq!(d.app.palette_rows()[0].label, "Quit");
    assert!(d.mouse(MouseEventKind::Down(MouseButton::Left), b.list.x + 4, b.list.y));
    assert!(!d.app.quit, "a click never quits at once");
    assert_eq!(d.app.mode(), Mode::Normal);
    let s = d.screen();
    assert!(s.contains("Quit slakio?") && s.contains("No message is waiting to be sent."), "{s}");
    d.keys("enter");
    assert!(!d.app.quit && d.app.overlay().is_none(), "Stay has the focus");
    // With text not sent, the same question as Ctrl+Q; y quits.
    d.open("backend");
    d.keys("i");
    d.type_text("half written");
    d.keys("esc ctrl+p");
    d.type_text("qa");
    assert!(d.mouse(MouseEventKind::Down(MouseButton::Left), b.list.x + 4, b.list.y));
    assert_eq!(d.app.overlay(), Some(Overlay::Dialog(Question::Quit)));
    d.keys("y");
    assert!(d.app.quit, "y quits");
    // Typed, `:qa` Enter is asked for by name: it quits as in vim; `:q` closes, never quits.
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.type_text("q");
    assert_eq!(d.app.palette_rows()[0].label, "Close the pane, then its tab");
    d.keys("enter");
    assert!(!d.app.quit);
    d.keys(":");
    d.type_text("qa");
    d.keys("enter");
    assert!(d.app.quit);
}

#[test]
fn the_help_opened_over_the_palette_is_drawn_on_top_and_takes_the_mouse() {
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.keys("f1");
    assert_eq!(d.app.overlay(), Some(Overlay::Help), "the keys go to the help");
    let s = d.screen();
    assert!(s.contains("Keys — Command line"), "the help shows: {s}");
    assert!(!s.contains("Tab/↑↓ select"), "the palette under it is not drawn over it: {s}");
    let selected = d.app.cmdline.selected;
    d.mouse(MouseEventKind::ScrollDown, 60, 20);
    assert_eq!(d.app.cmdline.selected, selected, "the wheel goes to the help, not the palette under it");
}
