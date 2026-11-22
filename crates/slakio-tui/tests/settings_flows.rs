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
use slakio_core::i18n::Lang;
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
    assert!(d.screen().contains("not saved, so not used on next launch"), "{}", d.screen());
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
            let key = SETTINGS[d.app.prefs.screen.as_ref().unwrap().selected].key;
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
