//! The settings screen (`Space ,`, `:settings`): its text at 80x24 and 120x40 in English and
//! Korean (masked) and on a short 60x20 screen where the list scrolls; the which-key entry; what
//! it saves in the config file (comments kept, the file the user's alone) and what a broken
//! config file is told; a theme file that cannot be used, shown but never kept; `:set` errors;
//! `:images` and `:avatars` in the keyboard help. Screens are insta snapshots; under CI insta
//! never rewrites them.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use slakio_core::config::{self, Config};
use slakio_core::i18n::{Label, Lang};
use slakio_tui::app::{Effect, Settings, WHICH_KEY_DELAY};
use slakio_tui::theme::{Background, Look};
use std::path::{Path, PathBuf};

const TRUECOLOR: Look = Look { truecolor: true, background: Background::Dark, no_color: false };

/// A scratch directory of its own, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("slakio-settings-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The demo at `w`×`h` in `lang`, its config file `path` (`writable` or not) read at launch.
fn booted(w: u16, h: u16, lang: Lang, cfg: Config, writable: bool) -> Demo {
    let mut d = Demo::with(w, h, lang, Settings::default());
    d.app.load_at_start(TRUECOLOR, &cfg, writable, d.now);
    d.app.prefs.download_env = None;
    d
}

/// A config file at a path that reads the same on every machine (never written).
fn shown_config() -> Config {
    Config { path: Some(PathBuf::from("/home/me/.config/slakio/config.toml")), ..Config::default() }
}

/// What the binary's loop does with the settings saved: `config::set` on the file (none when it
/// cannot be written).
fn save(d: &mut Demo, path: Option<&Path>) -> usize {
    let mut n = 0;
    for e in d.app.take_effects() {
        if let Effect::Save { key, value } = e {
            n += 1;
            if let Some(p) = path {
                config::set(p, key, &value).unwrap();
            }
        }
    }
    n
}

#[test]
fn the_screen_at_two_sizes_in_two_languages() {
    for (w, h) in [(80, 24), (120, 40)] {
        for (lang, tag) in [(Lang::En, "en"), (Lang::Ko, "ko")] {
            let mut d = booted(w, h, lang, shown_config(), true);
            d.keys("space ,");
            insta::assert_snapshot!(format!("settings_{tag}_{w}x{h}"), d.snap());
        }
    }
    // `:settings` opens the same screen.
    let mut d = booted(80, 24, Lang::En, shown_config(), true);
    d.command("settings");
    let by_command = d.snap();
    let mut d = booted(80, 24, Lang::En, shown_config(), true);
    d.keys("space ,");
    assert_eq!(by_command, d.snap());
}

#[test]
fn a_short_screen_scrolls_the_list_and_says_how_much_is_out_of_view() {
    let mut d = booted(60, 20, Lang::En, shown_config(), true);
    d.keys("space ,");
    insta::assert_snapshot!("settings_en_60x20_top", d.snap());
    d.keys("k");
    insta::assert_snapshot!("settings_en_60x20_bottom", d.snap());
    d.keys("j j j j j j");
    insta::assert_snapshot!("settings_en_60x20_middle", d.snap());
}

#[test]
fn a_screen_too_small_to_draw_it_changes_nothing() {
    let mut d = booted(120, 40, Lang::En, shown_config(), true);
    d.keys("space ,");
    d.app.resize(40, 8);
    d.keys("j l enter");
    assert!(!d.app.settings.compact, "nothing changes unseen");
    assert_eq!(save(&mut d, None), 0);
    d.keys("esc");
    assert!(d.app.prefs.screen.is_none(), "Esc still closes it");
}

#[test]
fn which_key_lists_the_settings_and_no_key_clashes() {
    for (lang, want) in [(Lang::En, ",    Settings"), (Lang::Ko, ",    ＊＊")] {
        let mut d = booted(120, 40, lang, shown_config(), true);
        d.keys("space");
        d.now += WHICH_KEY_DELAY;
        d.app.on_tick(d.now);
        assert!(d.snap().contains(want), "{lang:?}:\n{}", d.snap());
    }
    let conflicts = slakio_tui::keymap::check(slakio_tui::keymap::Keymap::default().all(), false);
    assert!(conflicts.is_empty(), "{conflicts:?}");
}

#[test]
fn a_change_is_saved_keeping_the_file_s_comments_and_privacy() {
    let dir = Scratch::new("save");
    let path = dir.0.join("config.toml");
    let text = "# my settings\ndensity = \"comfortable\"  # roomy\n\n# the folder\ndownload_max_mb = 300\n";
    std::fs::write(&path, text).unwrap();
    let (cfg, err) = config::load(Some(&path));
    assert_eq!(err, None);
    let mut d = booted(120, 40, Lang::En, cfg, true);
    d.keys("space ,");
    d.keys("j l");
    assert!(d.app.settings.compact, "changed at once");
    assert_eq!(save(&mut d, Some(&path)), 1);
    let want = "# my settings\ndensity = \"compact\"  # roomy\n\n# the folder\ndownload_max_mb = 300\n";
    assert_eq!(std::fs::read_to_string(&path).unwrap(), want, "only the value changed");
    assert!(d.screen().contains("custom (300)"), "a size of the file's own is kept:\n{}", d.screen());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the user's alone");
    }
    // `:set` changes the same state and the same file.
    d.keys("esc");
    d.command("set density=comfortable");
    assert!(!d.app.settings.compact);
    d.command("set restore=off");
    assert_eq!(save(&mut d, Some(&path)), 2);
    let (cfg, _) = config::load(Some(&path));
    assert_eq!((cfg.density.as_str(), cfg.restore.as_str(), cfg.download_max_mb), ("comfortable", "off", 300));
}

#[test]
fn a_broken_config_file_is_said_and_left_alone() {
    let dir = Scratch::new("broken");
    let path = dir.0.join("config.toml");
    let text = "density = \"tiny\"\n";
    std::fs::write(&path, text).unwrap();
    let (cfg, err) = config::load(Some(&path));
    assert!(err.is_some());
    let mut d = booted(120, 40, Lang::En, cfg, err.is_none());
    d.keys("space ,");
    let screen = d.screen();
    assert!(screen.contains("The config file has errors: changes are not saved and last until you quit"), "{screen}");
    d.keys("j l");
    assert!(d.app.settings.compact, "in memory");
    save(&mut d, None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "the file is untouched");
}

#[test]
fn a_broken_config_file_never_says_a_change_applies_on_next_launch() {
    let dir = Scratch::new("broken-restart");
    let path = dir.0.join("config.toml");
    std::fs::write(&path, "density = \"tiny\"\n").unwrap();
    let (cfg, err) = config::load(Some(&path));
    assert!(err.is_some());
    let mut d = booted(120, 40, Lang::En, cfg, false);
    d.keys("space ,");
    assert!(d.screen().contains("unsaved: not used next launch"), "{}", d.screen());
    // language
    d.keys("j j j j j j l");
    assert_eq!(d.app.prefs.config.language, "en");
    let screen = d.screen();
    assert!(!screen.contains("applies on next launch"), "{screen}");
    assert!(d.status_line().contains("language: en (not saved"), "{}", d.status_line());
    // A file that can be written: said as before.
    let mut d = booted(120, 40, Lang::En, shown_config(), true);
    d.keys("space ,");
    assert!(d.screen().contains("applies on next launch"), "{}", d.screen());
}

#[test]
fn a_theme_file_that_cannot_be_used_is_shown_never_kept() {
    let dir = Scratch::new("theme");
    std::fs::create_dir_all(dir.0.join("themes")).unwrap();
    std::fs::write(dir.0.join("themes").join("zbad.toml"), "[colors]\naccent = \"#12345\"\n").unwrap();
    std::fs::write(dir.0.join("themes").join("mine.toml"), "extends = \"nord\"\n").unwrap();
    let cfg = Config { path: Some(dir.0.join("config.toml")), ..Config::default() };
    let mut d = booted(120, 40, Lang::En, cfg, true);
    let before = d.app.theme.clone();
    d.keys("space ,");
    // `h` from `auto`, the first: round to the last, the theme file.
    d.keys("h");
    let screen = d.screen();
    let top = screen.lines().nth(2).unwrap();
    assert!(top.contains("Theme not used:"), "said on top:\n{screen}");
    let why = d.app.prefs.screen.as_ref().and_then(|s| s.preview.as_ref()).and_then(|p| p.error.clone());
    let why = why.map(|m| d.app.i18n.msg(&m).to_string()).unwrap_or_default();
    assert!(why.contains("zbad.toml:2: #12345 is not a colour"), "{why}");
    assert!(screen.contains("zbad") && screen.contains("A theme file"), "{screen}");
    assert_eq!(d.app.theme, before, "the theme before stays drawn");
    d.keys("enter");
    assert_eq!(d.app.theme_setting, "auto", "never kept");
    assert_eq!(save(&mut d, None), 0);
    // `l`, `l` from there: `auto`, then `terminal`; `Esc` gives the theme back.
    d.keys("l l");
    assert_ne!(d.app.theme, before);
    d.keys("esc");
    assert_eq!(d.app.theme, before);
    assert_eq!(save(&mut d, None), 0, "nothing shown was saved");
    // The palette's `:theme ` lists the theme files too, and runs one.
    d.keys(":");
    d.type_text("theme mine");
    let screen = d.screen();
    assert!(screen.lines().any(|l| l.contains("mine") && l.contains("A theme file")), "{screen}");
    d.keys("enter");
    assert_eq!(d.app.theme_setting, "mine");
    assert_eq!(save(&mut d, None), 1);
}

#[test]
fn set_errors_say_what_is_wrong_in_both_languages() {
    let keys = "theme, density, nav_rows, icons, avatars, images, language, downloads, download_max_mb, network, live, restore, cache";
    let cases = [
        ("set colour=red", format!("No setting colour (one of {keys})")),
        ("set live=maybe", "live cannot be maybe (one of on, off)".to_string()),
        ("set download_max_mb=0", "download_max_mb cannot be 0 (one of 1-100000)".to_string()),
        ("set cache", "Write :set cache= and a value".to_string()),
        (
            "set downloads=saved",
            "Not a folder slakio can use: saved (an absolute path, or one that starts with ~/)".to_string(),
        ),
    ];
    let ko = [
        format!("colour＊＊ ＊＊＊ ＊＊＊＊ ({keys} ＊ ＊＊)"),
        "live＊(＊) maybe＊ ＊ ＊＊＊＊ (on, off ＊ ＊＊)".to_string(),
        "download_max_mb＊(＊) 0＊ ＊ ＊＊＊＊ (1-100000 ＊ ＊＊)".to_string(),
        ":set cache= ＊＊ ＊＊ ＊＊＊".to_string(),
        "slakio＊ ＊ ＊ ＊＊ ＊＊: saved (＊＊ ＊＊＊ ~/＊ ＊＊＊＊ ＊＊)".to_string(),
    ];
    for (i, (line, en)) in cases.iter().enumerate() {
        for (lang, want) in [(Lang::En, en.clone()), (Lang::Ko, ko[i].clone())] {
            let mut d = booted(120, 40, lang, shown_config(), true);
            d.command(line);
            let said = d.app.cmdline.error.as_ref().map(|m| demo::mask_hangul(&d.app.i18n.msg(m)));
            assert_eq!(said.as_deref(), Some(want.as_str()), "{lang:?} {line}");
            assert_eq!(save(&mut d, None), 0);
        }
    }
    // The aliases keep their own words.
    let mut d = booted(120, 40, Lang::En, shown_config(), true);
    // `:set` takes a whole value only: a part of one is not a fuzzy pick.
    for line in ["set density=c", "set icons=o", "set avatars=ini", "set images=au"] {
        d.command(line);
        assert!(d.app.cmdline.error.is_some(), "{line}");
        d.keys("esc");
    }
    assert!(!d.app.settings.compact);
    assert_eq!(save(&mut d, None), 0);
    d.command("set density=tiny");
    let said = d.app.cmdline.error.as_ref().map(|m| d.app.i18n.msg(m).to_string());
    assert_eq!(said.as_deref(), Some("Unknown density tiny (one of comfortable, compact)"));
    // A good one: done, saved, the palette closed.
    d.keys("esc");
    d.command("set downloads=~/x");
    assert!(!d.app.cmdline.is_open());
    assert_eq!(d.app.prefs.config.downloads, "~/x");
    assert_eq!(save(&mut d, None), 1);
}

#[test]
fn the_help_lists_images_avatars_and_the_settings_key() {
    for (word, want) in [("image", ":images"), ("avatar", ":avatars"), ("settings", "Space ,")] {
        let mut d = booted(120, 40, Lang::En, shown_config(), true);
        d.keys("?");
        d.keys("/");
        d.type_text(word);
        assert!(d.screen().contains(want), "{word}:\n{}", d.screen());
    }
}

#[test]
fn the_out_of_view_counts_never_cover_a_row() {
    use slakio_tui::app::settings::table::SETTINGS;
    for (w, h) in [(80, 24), (60, 20)] {
        let mut d = booted(w, h, Lang::En, shown_config(), true);
        d.keys("space ,");
        for _ in 0..=SETTINGS.len() {
            // A row is named by its label.
            let key = d.app.i18n.label(SETTINGS[d.app.prefs.screen.as_ref().unwrap().selected].label).to_string();
            let screen = d.screen();
            let row = screen.lines().find(|l| l.contains(&format!(" {key} ")) && l.contains('\u{2039}'));
            let row = row.unwrap_or_else(|| panic!("{w}x{h} {key}: no row\n{screen}"));
            assert!(row.contains('\u{203A}') && !row.contains(" more "), "{w}x{h} {key}: {row}\n{screen}");
            for l in screen.lines().filter(|l| l.contains(" more ")) {
                assert!(!l.contains('\u{2039}'), "{w}x{h}: a count on a row: {l}\n{screen}");
            }
            d.keys("j");
        }
    }
}

#[test]
fn the_status_line_offers_no_pane_keys_under_the_screen() {
    // As under the keyboard help: the place, no keys of the pane under the screen.
    let line =
        |place: &str, gap: usize, mark: &str| format!(" NORMAL  ▌A company › {place}{} │ {mark}", " ".repeat(gap));
    let want = [line("Home", 46, "demo"), line("Home", 86, "demo"), line("＊", 48, "＊＊"), line("＊", 88, "＊＊")];
    let mut lines = Vec::new();
    for (w, lang) in [(80, Lang::En), (120, Lang::En), (80, Lang::Ko), (120, Lang::Ko)] {
        let mut d = booted(w, 24, lang, shown_config(), true);
        d.keys("space ,");
        let status = demo::mask_hangul(&d.status_line());
        assert!(!status.contains("Enter") && !status.contains("help"), "{w} {lang:?}: {status}");
        lines.push(status);
    }
    assert_eq!(lines, want, "at 80 and 120 columns, in English and Korean");
}

#[test]
fn the_frame_says_the_keys_of_the_folder_and_the_name_typed() {
    // The settings box's bottom border, from its corner to its corner.
    let border = |d: &Demo| {
        let screen = d.snap();
        let line = screen.lines().find(|l| l.contains("Esc") && l.contains('╰')).unwrap_or_default().to_string();
        let from = line.find('╰').unwrap_or(0);
        let to = line.rfind('╯').map_or(line.len(), |i| i + '╯'.len_utf8());
        line[from..to].to_string()
    };
    let mut got = Vec::new();
    for (w, h) in [(80, 24), (120, 40)] {
        for lang in [Lang::En, Lang::Ko] {
            let mut d = booted(w, h, lang, shown_config(), true);
            d.keys("space ,");
            let list = border(&d);
            // downloads: type a folder.
            d.keys("j j j j j j j enter");
            assert!(d.app.prefs.screen.as_ref().is_some_and(|s| s.folder.is_some()), "{w} {lang:?}");
            let folder = border(&d);
            d.keys("esc");
            assert_eq!(border(&d), list, "{w} {lang:?}: the list's keys again");
            // theme: the name of a copy.
            d.keys("k k k k k k k c");
            assert!(d.app.prefs.screen.as_ref().is_some_and(|s| s.name.is_some()), "{w} {lang:?}");
            assert_eq!(border(&d), folder, "{w} {lang:?}: the same keys");
            got.push(folder);
        }
    }
    let line = |w: usize, keys: &str, kw: usize| format!("╰{} {keys} ╯", "─".repeat(w - kw - 4));
    let (en, ko) = ("Enter save · Esc cancel · Ctrl+U clear", "Enter ＊＊ · Esc ＊＊ · Ctrl+U ＊＊＊");
    let want = [line(76, en, 38), line(76, ko, 37), line(100, en, 38), line(100, ko, 37)];
    assert_eq!(got, want, "at 80 and 120 columns, in English and Korean");
}

#[test]
fn the_help_line_of_login_is_whole_at_80_and_120_columns() {
    let mut got = Vec::new();
    for w in [80, 120] {
        for lang in [Lang::En, Lang::Ko] {
            let mut d = booted(w, 24, lang, shown_config(), true);
            d.keys("?");
            d.keys("/");
            d.type_text("login");
            let screen = d.snap();
            let line = screen.lines().find(|l| l.contains(":login")).unwrap_or_default();
            got.push(line.split('│').filter(|c| c.contains(":login")).collect::<String>().trim().to_string());
        }
    }
    let (en, ko) =
        ("Sign in again: a workspace Slack signed out, or another", "＊＊ ＊＊＊: ＊＊＊＊＊ ＊＊＊＊＊＊＊ ＊＊ ＊");
    let line = |text: &str, gap: usize| format!("{text}{}:login", " ".repeat(gap));
    let want = [line(en, 9), line(ko, 18), line(en, 33), line(ko, 42)];
    assert_eq!(got, want, "whole at 80 and 120 columns, in English and Korean");
}

/// The downloads row's label as the screen shows it (Hangul masked).
fn folder_label(d: &Demo) -> String {
    use slakio_tui::app::settings::table::{SETTINGS, find};
    demo::mask_hangul(&d.app.i18n.label(SETTINGS[find("downloads").unwrap()].label))
}

/// The value on the row labelled `key`, between its marks.
fn value_of(screen: &str, key: &str) -> String {
    let row = screen.lines().find(|l| l.split_once('‹').is_some_and(|(name, _)| name.trim_end().ends_with(key)));
    let row = row.unwrap_or_default();
    let from = row.find('‹').map_or(0, |i| i + '‹'.len_utf8());
    let to = row.find('›').unwrap_or(from);
    row[from..to].to_string()
}

#[test]
fn a_long_folder_is_cut_by_its_middle_never_widening_the_column() {
    let long = "/a/folder/name/long/enough/to/fill/the/value/column/by/itself/slakio";
    let mut got = Vec::new();
    for (w, h) in [(80, 24), (120, 40)] {
        for lang in [Lang::En, Lang::Ko] {
            let mut d = booted(w, h, lang, shown_config(), true);
            // The downloads row selected, so it is in view.
            d.keys("space , j j j j j j j");
            let before = d.snap();
            d.app.prefs.config.downloads = long.to_string();
            let screen = d.snap();
            got.push(value_of(&screen, &folder_label(&d)));
            // Every other row is as with the default folder.
            let label = folder_label(&d);
            let named = |l: &str| l.split_once('‹').is_some_and(|(name, _)| name.trim_end().ends_with(&label));
            let others = |s: &str| s.lines().filter(|l| !named(l)).collect::<Vec<_>>().join("\n");
            assert_eq!(others(&screen), others(&before), "{w} {lang:?}");
        }
    }
    let cut = " /a/folde…lf/slakio ";
    assert_eq!(got, [cut, cut, cut, cut], "at 80 and 120 columns, in English and Korean");
}

#[test]
fn the_read_only_folder_reads_in_the_muted_colour() {
    let mut got = Vec::new();
    for (w, h) in [(80, 24), (120, 40)] {
        for lang in [Lang::En, Lang::Ko] {
            let mut d = booted(w, h, lang, shown_config(), true);
            d.app.prefs.download_env = Some("/srv/files".to_string());
            d.command("theme nord");
            // The row under it selected: the folder's row in view, not under the selection bar.
            d.keys("space , j j j j j j j j");
            let screen = d.snap();
            let label = folder_label(&d);
            let y = screen
                .lines()
                .position(|l| l.split_once('‹').is_some_and(|(name, _)| name.trim_end().ends_with(&label)))
                .unwrap();
            let buf = d.buffer();
            let x = (0..w).find(|&x| buf[(x, y as u16)].symbol() == "‹").unwrap();
            for dx in 0..14 {
                assert_eq!(buf[(x + dx, y as u16)].fg, d.app.theme.fg_muted, "{w} {lang:?} at {dx}");
            }
            got.push(value_of(&d.snap(), &folder_label(&d)));
        }
    }
    let shown = " /srv/files         ";
    assert_eq!(got, [shown, shown, shown, shown], "at 80 and 120 columns, in English and Korean");
}

#[test]
fn a_theme_file_that_cannot_be_used_says_so_on_its_row() {
    let mut got = Vec::new();
    for (w, h) in [(80, 24), (120, 40)] {
        for lang in [Lang::En, Lang::Ko] {
            let dir = Scratch::new(&format!("broken-row-{w}"));
            std::fs::create_dir_all(dir.0.join("themes")).unwrap();
            std::fs::write(dir.0.join("themes").join("zbad.toml"), "[colors]\naccent = \"#12345\"\n").unwrap();
            // Set at launch, and shown on the theme row.
            let cfg = Config { path: Some(dir.0.join("config.toml")), theme: "zbad".to_string(), ..Config::default() };
            let mut d = booted(w, h, lang, cfg, true);
            d.keys("space ,");
            let screen = d.snap();
            let set = screen.lines().find(|l| l.contains(" zbad ")).unwrap_or_default().to_string();
            d.keys("l h");
            assert_eq!(d.snap().lines().find(|l| l.contains(" zbad ")).unwrap_or_default(), set, "{w} {lang:?}");
            got.push(
                set.split('›').nth(1).unwrap_or_default().split('│').next().unwrap_or_default().trim().to_string(),
            );
        }
    }
    let (en, ko) = ("A theme file with an error", "＊＊＊ ＊＊ ＊＊ ＊＊");
    assert_eq!(got, [en, ko, en, ko], "whole at 80 and 120 columns, in English and Korean");
}

#[test]
fn the_help_line_of_the_palette_is_whole_at_80_and_120_columns() {
    let mut got = Vec::new();
    for w in [80, 120] {
        for lang in [Lang::En, Lang::Ko] {
            let mut d = booted(w, 24, lang, shown_config(), true);
            d.keys("?");
            d.keys("/");
            d.type_text("ctrl+p");
            let screen = d.snap();
            let line =
                screen.lines().find(|l| l.ends_with("Ctrl+P │ │") || l.contains("Ctrl+P │  ")).unwrap_or_default();
            got.push(line.split('│').filter(|c| c.contains("Ctrl+P")).collect::<String>().trim().to_string());
        }
    }
    let (en, ko) = ("Command palette: every command, with its keys", "＊＊ ＊＊＊: ＊＊ ＊＊＊ ＊＊, ＊＊ ＊＊");
    let line = |text: &str, gap: usize| format!("{text}{}Ctrl+P", " ".repeat(gap));
    let want = [line(en, 19), line(ko, 24), line(en, 43), line(ko, 48)];
    assert_eq!(got, want, "whole at 80 and 120 columns, in English and Korean");
}

#[test]
fn the_korean_size_text_needs_no_particle_after_a_number() {
    // The screen is too small to show 80 or 120 columns of it: the text at two small sizes.
    let mut got = Vec::new();
    for (w, h) in [(40, 8), (45, 9)] {
        for lang in [Lang::En, Lang::Ko] {
            let mut d = booted(80, 24, lang, shown_config(), true);
            d.app.resize(w, h);
            let text: Vec<String> = d
                .snap()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.contains("NORMAL"))
                .map(String::from)
                .collect();
            got.push(text.join(" "));
        }
    }
    // In Korean a word follows the size, never a particle on the number.
    let en = |size: &str| format!("The terminal is {size}; slakio needs at least 50×10.");
    let ko = |size: &str| format!("＊＊＊＊ {size}＊＊＊. slakio＊ ＊＊ 50×10 ＊＊＊ ＊＊＊＊＊.");
    assert_eq!(got, [en("40×8"), ko("40×8"), en("45×9"), ko("45×9")]);
}

#[test]
fn rows_are_named_by_label_and_the_selected_one_says_its_set_key() {
    use slakio_tui::app::settings::table::SETTINGS;
    let mut got = Vec::new();
    for (w, h) in [(80, 24), (120, 40)] {
        for lang in [Lang::En, Lang::Ko] {
            let mut d = booted(w, h, lang, shown_config(), true);
            d.keys("space ,");
            let mut names = Vec::new();
            for spec in SETTINGS {
                let screen = d.snap();
                // The selected row's name, whole, before its value.
                let label = demo::mask_hangul(&d.app.i18n.label(spec.label));
                let row =
                    screen.lines().find(|l| l.split_once('‹').is_some_and(|(n, _)| n.trim_end().ends_with(&label)));
                assert!(row.is_some(), "{w} {lang:?} {}: no row named {label}\n{screen}", spec.key);
                names.push(label);
                // Under the list, the key `:set` takes, whole.
                let typed = format!(":set {}=…", spec.key);
                let set = screen.lines().find(|l| l.contains(&typed)).unwrap_or_default();
                let set = set.split('│').find(|c| c.contains(&typed)).unwrap_or_default().trim().to_string();
                let want = match lang {
                    Lang::Ko => format!("＊＊＊＊ ＊＊＊: :set {}=…", spec.key),
                    _ => format!("Set with :set {}=…", spec.key),
                };
                assert_eq!(set, want, "{w} {lang:?}");
                d.keys("j");
            }
            got.push(names.join(" | "));
        }
    }
    let en = "Theme | Density | View rows | Nerd Font icons | Avatars | Images | Language | Download folder | \
              Download limit | Network | Live updates | Restore tabs | Message cache";
    let ko = "＊＊ | ＊＊ | ＊＊ ＊ | Nerd Font ＊＊＊ | ＊＊＊ | ＊＊＊ | ＊＊ ＊＊ | ＊＊＊＊ ＊＊ | ＊＊＊＊ ＊＊ | ＊＊＊＊ | \
              ＊＊＊ ＊＊＊＊ | ＊ ＊＊ | ＊＊＊ ＊＊";
    assert_eq!(got, [en, ko, en, ko], "at 80 and 120 columns, in English and Korean");
    // The keys stay findable: `:images` and `:avatars` in the help.
    for word in [":images", ":avatars"] {
        let mut d = booted(80, 24, Lang::Ko, shown_config(), true);
        d.keys("?");
        d.keys("/");
        d.type_text(&word[1..]);
        assert!(d.snap().contains(word), "{word}:\n{}", d.snap());
    }
}

/// The screen's lines that are a group's heading, each with the line under it.
fn headings_and_next(d: &Demo) -> Vec<(String, String)> {
    let groups: Vec<String> = [
        Label::SettingsGroupDisplay,
        Label::SettingsGroupPictures,
        Label::SettingsGroupLanguage,
        Label::SettingsGroupFiles,
        Label::SettingsGroupNetwork,
    ]
    .iter()
    .map(|l| d.app.i18n.label(*l).to_string())
    .collect();
    let lines: Vec<String> = d.screen().lines().map(str::to_string).collect();
    // The settings box's inside, without the panes' and the box's borders.
    let inside = |line: &str| line.trim_matches(|c: char| "│├┤ ▎".contains(c)).to_string();
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| groups.contains(&inside(l)))
        .map(|(i, l)| (l.clone(), lines.get(i + 1).cloned().unwrap_or_default()))
        .collect()
}

/// A group's heading is never alone at the bottom of the list (as "Files" was above "7 more
/// below" at 80x24): wherever the list is scrolled to, at every height, in both languages, the
/// line under a heading is a row of it.
#[test]
fn a_heading_is_never_alone_at_the_bottom_of_the_list() {
    let rows = slakio_tui::app::settings::table::SETTINGS.len();
    for (lang, sizes) in [(Lang::En, 10..=30), (Lang::Ko, 10..=30)] {
        for h in sizes {
            for w in [60, 80] {
                let mut d = booted(w, h, lang, shown_config(), true);
                d.keys("space ,");
                for step in 0..=rows {
                    let shown = headings_and_next(&d);
                    for (heading, next) in &shown {
                        assert!(
                            next.contains('‹'),
                            "{lang:?} {w}x{h} step {step}: {heading:?} has no row under it:\n{}",
                            d.screen()
                        );
                    }
                    d.keys("j");
                }
            }
        }
    }
    // At 80x24 the first screen shows every group's heading it shows with its first row.
    for lang in [Lang::En, Lang::Ko] {
        let mut d = booted(80, 24, lang, shown_config(), true);
        d.keys("space ,");
        let shown = headings_and_next(&d);
        assert!(shown.len() >= 3, "{lang:?}:\n{}", d.screen());
        assert!(shown.iter().all(|(_, next)| next.contains('‹')), "{lang:?}:\n{}", d.screen());
    }
}
