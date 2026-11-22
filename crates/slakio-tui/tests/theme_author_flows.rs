//! Writing theme files from the app: `c` on the settings screen's theme row (its name prompt at
//! 80x24 and 120x40, English and Korean, masked), and `:theme copy`, `:theme new` and
//! `:theme update` in the palette. A copy or a new file is drawn with and saved as `theme=`; a
//! name that is taken or not valid is said with its exact words and nothing is written.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use slakio_core::config::Config;
use slakio_core::i18n::Lang;
use slakio_tui::app::{Effect, Settings};
use slakio_tui::theme::{Background, Look};
use std::path::PathBuf;

const TRUECOLOR: Look = Look { truecolor: true, background: Background::Dark, no_color: false };

/// A scratch config folder of its own, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("slakio-author-flows-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn config(&self) -> Config {
        Config { path: Some(self.0.join("config.toml")), ..Config::default() }
    }

    fn theme(&self, name: &str) -> PathBuf {
        self.0.join("themes").join(format!("{name}.toml"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn booted(w: u16, h: u16, lang: Lang, cfg: Config) -> Demo {
    let mut d = Demo::with(w, h, lang, Settings::default());
    d.app.load_at_start(TRUECOLOR, &cfg, true, d.now);
    d.app.prefs.download_env = None;
    d
}

/// A config file at a path that reads the same on every machine (nothing is written there:
/// the screens below only type a name).
fn shown_config() -> Config {
    Config { path: Some(PathBuf::from("/home/me/.config/slakio/config.toml")), ..Config::default() }
}

/// The `theme` values saved since the last call.
fn saved_themes(d: &mut Demo) -> Vec<String> {
    d.app
        .take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Save { key: "theme", value } => Some(value),
            _ => None,
        })
        .collect()
}

#[test]
fn the_name_prompt_at_two_sizes_in_two_languages() {
    for (w, h) in [(80, 24), (120, 40)] {
        for (lang, tag) in [(Lang::En, "en"), (Lang::Ko, "ko")] {
            let mut d = booted(w, h, lang, shown_config());
            d.keys("space , l c");
            insta::assert_snapshot!(format!("theme_copy_name_{tag}_{w}x{h}"), d.snap());
            // A built-in theme's name is refused, said on top.
            d.keys("ctrl+u");
            d.type_text("Nord");
            d.keys("enter");
            insta::assert_snapshot!(format!("theme_copy_name_taken_{tag}_{w}x{h}"), d.snap());
        }
    }
}

#[test]
fn c_on_the_theme_row_copies_the_theme_shown_and_draws_with_it() {
    let dir = Scratch::new("c");
    let mut d = booted(120, 40, Lang::En, dir.config());
    // `l` shows `terminal`; `c` asks for the name of its copy.
    d.keys("space , l c");
    let screen = d.screen();
    assert!(screen.contains("Copy to: terminal-copy"), "{screen}");
    assert!(screen.contains("A copy of terminal: Enter writes it and uses it, Esc cancels"), "{screen}");
    // `Esc` cancels: nothing written, the preview still shown.
    d.keys("esc");
    assert!(!dir.0.join("themes").exists());
    assert!(d.app.prefs.screen.as_ref().is_some_and(|s| s.preview.is_some() && s.name.is_none()));
    d.keys("c enter");
    let path = dir.theme("terminal-copy");
    assert!(std::fs::read_to_string(&path).unwrap().contains("extends = \"terminal\"\n"));
    assert_eq!(d.app.theme_setting, "terminal-copy");
    assert_eq!(saved_themes(&mut d), vec!["terminal-copy".to_string()]);
    assert_eq!(d.app.theme.name, "terminal-copy");
    let s = d.app.prefs.screen.as_ref().unwrap();
    assert!(s.preview.is_none() && s.name.is_none() && s.themes.contains(&"terminal-copy".to_string()));
    // The theme row shows the copy; `Esc` closes and keeps it drawn.
    assert!(d.screen().contains("terminal-copy"));
    d.keys("esc");
    assert_eq!(d.app.theme.name, "terminal-copy");
    // Again: the next free name, and a taken one typed is refused with the file kept.
    d.keys("space , c");
    assert!(d.screen().contains("Copy to: terminal-copy-copy"), "{}", d.screen());
    d.keys("ctrl+u");
    d.type_text("terminal-copy");
    d.keys("enter");
    let want = format!("{} is there already: it is left as it is", path.display());
    let why = d.app.prefs.screen.as_ref().and_then(|s| s.name.as_ref()).and_then(|(_, e)| e.clone());
    assert_eq!(why.map(|m| d.app.i18n.msg(&m).to_string()), Some(want));
    assert!(std::fs::read_to_string(&path).unwrap().contains("extends = \"terminal\"\n"));
    assert!(saved_themes(&mut d).is_empty());
}

#[test]
fn the_palette_copies_writes_and_updates_theme_files() {
    let dir = Scratch::new("palette");
    let mut d = booted(120, 40, Lang::En, dir.config());
    // The row says what Enter does.
    d.keys(":");
    d.type_text("theme copy dark mine");
    assert!(d.screen().contains("Copy dark to the theme file mine and use it"), "{}", d.screen());
    d.keys("enter");
    assert_eq!(d.app.theme_setting, "mine");
    assert_eq!(saved_themes(&mut d), vec!["mine".to_string()]);
    assert!(d.status_line().contains("Theme mine: written to"), "{}", d.status_line());
    let text = std::fs::read_to_string(dir.theme("mine")).unwrap();
    // Taken: said in the palette with the exact words, the file kept.
    d.command("theme copy nord mine");
    let taken = format!("{} is there already: it is left as it is", dir.theme("mine").display());
    assert_eq!(d.app.cmdline.error.as_ref().map(|m| d.app.i18n.msg(m).to_string()), Some(taken));
    assert_eq!(std::fs::read_to_string(dir.theme("mine")).unwrap(), text);
    d.keys("esc");
    // Bad names: exact words, nothing written.
    for (cmd, want) in [
        ("theme new ../x", "../x is not a theme file's name: 1 to 64 ASCII letters, digits, _ and - (A-Z a-z 0-9 _ -)"),
        ("theme copy dark Dracula", "Dracula is the name of a built-in theme"),
        ("theme new nul", "nul is not a theme file's name: 1 to 64 ASCII letters, digits, _ and - (A-Z a-z 0-9 _ -)"),
        ("theme copy", "Use :theme copy <from> [name], :theme new <name> or :theme update [name]"),
        ("theme update dark", "dark is a built-in theme: there is no file to update"),
    ] {
        d.command(cmd);
        assert_eq!(d.app.cmdline.error.as_ref().map(|m| d.app.i18n.msg(m).to_string()).as_deref(), Some(want));
        d.keys("esc");
    }
    assert!(saved_themes(&mut d).is_empty());
    // `:theme new` extends the built-in theme drawn before a file: `terminal` here.
    d.command("theme new fresh");
    assert!(std::fs::read_to_string(dir.theme("fresh")).unwrap().contains("extends = \"terminal\"\n"));
    assert_eq!(d.app.theme_setting, "fresh");
    // `:theme update` (the current one): nothing to add.
    d.command("theme update");
    assert!(d.status_line().contains("Every token is listed already in"), "{}", d.status_line());
    // `:theme <name>` still switches.
    d.command("theme mine");
    assert_eq!(d.app.theme_setting, "mine");
}

#[test]
fn no_config_folder_writes_nothing() {
    let mut d = booted(120, 40, Lang::En, Config::default());
    d.command("theme copy dark mine");
    let why = d.app.cmdline.error.as_ref().map(|m| d.app.i18n.msg(m).to_string());
    assert_eq!(why.as_deref(), Some("No config folder: there is no themes folder to write to"));
}

#[cfg(unix)]
#[test]
fn a_shared_themes_folder_is_said_and_not_written_in() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("shared");
    let shared = dir.0.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
    std::os::unix::fs::symlink(&shared, dir.0.join("themes")).unwrap();
    let mut d = booted(120, 40, Lang::En, dir.config());
    d.command("theme copy dark mine");
    let want = format!(
        "{} is shared (not yours alone, or others may write in it): no theme file is written there",
        dir.0.join("themes").display()
    );
    assert_eq!(d.app.cmdline.error.as_ref().map(|m| d.app.i18n.msg(m).to_string()), Some(want));
    assert!(!dir.theme("mine").exists());
    assert!(saved_themes(&mut d).is_empty());
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn the_palette_lists_the_theme_files_once_while_it_is_open() {
    let dir = Scratch::new("listing-once");
    std::fs::create_dir_all(dir.0.join("themes")).unwrap();
    std::fs::write(dir.theme("first"), "extends = \"dark\"\n").unwrap();
    let mut d = booted(120, 40, Lang::En, dir.config());
    let listed = |d: &Demo, name: &str| d.app.palette_rows().iter().any(|r| r.name == name);
    d.keys(":");
    d.type_text("theme ");
    assert!(listed(&d, "first"), "{}", d.screen());
    // A file made while the palette is open is not read on the next key or frame.
    std::fs::write(dir.theme("second"), "extends = \"dark\"\n").unwrap();
    d.type_text("s");
    d.keys("backspace");
    let _ = d.screen();
    assert!(!listed(&d, "second"), "the folder is read once while the palette is open");
    // Opened again: read again.
    d.keys("esc");
    d.keys(":");
    d.type_text("theme ");
    assert!(listed(&d, "second"), "{}", d.screen());
}

#[test]
fn typed_names_are_said_safe_to_print() {
    let dir = Scratch::new("typed-names");
    let mut d = booted(120, 40, Lang::En, dir.config());
    for typed in
        ["theme copy \u{202e}dark", "theme copy dark \u{202e}x", "theme new \u{202e}y", "theme update \u{202e}z"]
    {
        d.keys(":");
        d.type_text(typed);
        let labels: Vec<String> = d.app.palette_rows().into_iter().map(|r| r.label).collect();
        assert!(!labels.is_empty() && labels.iter().all(|l| !l.contains('\u{202e}')), "{typed:?}: {labels:?}");
        d.keys("esc");
    }
    d.command("set live=\u{202e}maybe");
    let said = d.app.cmdline.error.as_ref().map(|m| d.app.i18n.msg(m).to_string()).unwrap_or_default();
    assert!(said.starts_with("live cannot be maybe (one of"), "{said}");
}

#[test]
fn theme_new_extends_what_auto_picks_never_no_color_nor_a_theme_only_shown() {
    let dir = Scratch::new("new-auto");
    let no_color = Look { no_color: true, ..TRUECOLOR };
    let mut d = Demo::with(120, 40, Lang::En, Settings::default());
    d.app.load_at_start(no_color, &dir.config(), true, d.now);
    assert_eq!(d.app.theme_setting, "auto");
    d.command("theme new plain");
    assert_eq!(d.app.cmdline.error.as_ref().map(|m| d.app.i18n.msg(m).to_string()), None);
    let picked = slakio_tui::theme::resolve("auto", true, Background::Dark).name.to_string();
    let want = format!("extends = \"{picked}\"\n");
    assert!(std::fs::read_to_string(dir.theme("plain")).unwrap().contains(&want), "{want}");
    // A theme drawn that is not the one set (as one only shown is): the setting decides.
    let mut d = booted(120, 40, Lang::En, dir.config());
    d.app.theme = TRUECOLOR.theme("dracula");
    d.command("theme new other");
    assert!(std::fs::read_to_string(dir.theme("other")).unwrap().contains(&want), "{want}");
}

#[test]
fn a_terminal_file_with_a_hex_colour_still_marks_the_row_of_a_panel_without_focus() {
    let dir = Scratch::new("unfocused");
    std::fs::create_dir_all(dir.0.join("themes")).unwrap();
    std::fs::write(dir.theme("mine"), "[colors]\naccent = \"#ff8800\"\n").unwrap();
    let mut d = booted(80, 24, Lang::En, dir.config());
    d.command("theme mine");
    assert_eq!(d.app.theme.kind, slakio_tui::theme::Kind::Truecolor);
    let inner = d.app.list_parts().unwrap().rows;
    let y = inner.y + (d.app.list_cursor() - d.app.list_top()) as u16;
    d.keys("l tab");
    assert_ne!(d.app.focus(), slakio_tui::app::Focus::List, "the list panel has no focus");
    let buf = d.buffer();
    let bar = (inner.left()..inner.right()).any(|x| buf[(x, y)].bg != ratatui::style::Color::Reset);
    assert!(bar || buf[(inner.x, y)].symbol() == "\u{258E}", "the selected row is marked:\n{}", d.screen());
}

#[test]
fn the_name_rule_says_ascii_letters_on_the_screen() {
    let mut got = Vec::new();
    for (w, h) in [(80, 24), (120, 40)] {
        for lang in [Lang::En, Lang::Ko] {
            let dir = Scratch::new("ascii");
            let mut d = booted(w, h, lang, dir.config());
            d.keys("space , c ctrl+u");
            d.type_text("a.b");
            d.keys("enter");
            let screen = d.snap();
            // What is said on top of the box: its lines down to the first group's heading.
            let mut top: Vec<String> = screen
                .lines()
                .skip(2)
                .take_while(|l| !l.contains('‹'))
                .map(|l| l.split('│').nth(2).unwrap_or_default().trim().to_string())
                .collect();
            top.pop();
            got.push(top.join(" "));
            assert!(!dir.theme("a.b").exists());
        }
    }
    let en = "a.b is not a theme file's name: 1 to 64 ASCII letters, digits, _ and - (A-Z a-z 0-9 _ -)";
    let ko = "a.b: ＊＊ ＊＊ ＊＊＊ ＊ ＊ ＊＊＊＊ (ASCII ＊＊＊, ＊＊, _, -＊ 1~64＊: A-Z a-z 0-9 _ -)";
    assert_eq!(got, [en, ko, en, ko], "whole at 80 and 120 columns, in English and Korean");
}
