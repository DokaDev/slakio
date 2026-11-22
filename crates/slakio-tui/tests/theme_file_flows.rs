//! Theme files while the app runs: at launch a theme file that cannot be used draws `terminal`
//! and the status line says why (fitted to its width, in English and Korean) while the config
//! file keeps the name; `:theme <file>` and `:set theme=<file>` draw with a good one and save
//! it, and say why a bad one is not used. One test: it works in a folder of its own (the
//! process's current folder), so the paths said are short and the same on every machine.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::style::Color;
use slakio_core::i18n::Lang;
use slakio_tui::app::{Effect, Settings};
use slakio_tui::theme::{Background, Kind, Look};
use std::path::Path;

const TRUECOLOR: Look = Look { truecolor: true, background: Background::Dark, no_color: false };

const CONFIG: &str = "# my settings\ntheme = \"broken\"\n";

fn saved(d: &mut Demo) -> Vec<String> {
    d.app
        .take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Save { key: "theme", value } => Some(value),
            _ => None,
        })
        .collect()
}

/// `text` with the paths written as this system writes them (`themes\x` on Windows).
fn native(text: &str) -> String {
    text.replace("themes/", &format!("themes{}", std::path::MAIN_SEPARATOR))
}

fn booted(w: u16, h: u16, lang: Lang) -> Demo {
    let mut d = Demo::with(w, h, lang, Settings::default());
    let (cfg, err) = slakio_core::config::load(Some(Path::new("config.toml")));
    assert!(err.is_none(), "{err:?}");
    assert_eq!(cfg.theme, "broken", "the config takes a theme file's name");
    d.app.load_at_start(TRUECOLOR, &cfg, true, d.now);
    d
}

#[test]
fn theme_files_at_launch_and_while_running() {
    let dir = std::env::temp_dir().join(format!("slakio-theme-files-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    std::fs::write("config.toml", CONFIG).unwrap();
    std::fs::write("themes/broken.toml", "extends = \"dark\"\n[colors]\naccent = \"#12345\"\n").unwrap();

    // The file's name (the theme's name is its stem) and why, whole: the folder is on the
    // settings screen.
    let want = [
        " NORMAL  ▌Home │ Theme not used: broken.toml:3: #12345 is not a colour   │ demo",
        " NORMAL  ▌A compa… › Home │ Theme not used: broken.toml:3: #12345 is not a colour │ Enter open · l peek · ? help │ demo",
        " NORMAL  ▌＊ │ ＊＊＊ ＊＊ ＊＊: broken.toml:3: #12345: ＊＊ ＊＊＊＊    │ ＊＊",
        " NORMAL  ▌A company › ＊ │ ＊＊＊ ＊＊ ＊＊: broken.toml:3: #12345: ＊＊ ＊＊＊＊        │ Enter ＊＊ · ? ＊＊＊ │ ＊＊",
    ];
    let mut lines = Vec::new();
    for (w, lang) in [(80, Lang::En), (120, Lang::En), (80, Lang::Ko), (120, Lang::Ko)] {
        let mut d = booted(w, 24, lang);
        assert_eq!(d.app.theme.name, "terminal", "{w} {lang:?}: a theme that cannot be used draws terminal");
        assert_eq!(d.app.theme_setting, "broken", "the setting stays as written");
        assert!(saved(&mut d).is_empty(), "nothing is saved");
        lines.push(demo::mask_hangul(d.status_line().trim_end()));
    }
    assert_eq!(lines, want, "the status line at 80 and 120 columns, in English and Korean");
    // However long the folder: the same words.
    let deep = dir.join("a-folder-name-long-enough-to-fill-the-status-line-on-its-own").join("and-more");
    std::fs::create_dir_all(deep.join("themes")).unwrap();
    std::fs::copy("themes/broken.toml", deep.join("themes").join("broken.toml")).unwrap();
    let cfg = slakio_core::config::Config {
        theme: "broken".to_string(),
        path: Some(deep.join("config.toml")),
        ..Default::default()
    };
    for (i, (w, lang)) in [(80, Lang::En), (120, Lang::En), (80, Lang::Ko), (120, Lang::Ko)].into_iter().enumerate() {
        let mut d = Demo::with(w, 24, lang, Settings::default());
        d.app.load_at_start(TRUECOLOR, &cfg, true, d.now);
        assert_eq!(demo::mask_hangul(d.status_line().trim_end()), want[i], "{w} {lang:?}: a deep folder");
        // The settings screen says the whole path.
        d.keys("space ,");
        let problem = d.app.prefs.screen.as_ref().and_then(|s| s.problem.clone());
        let said = problem.map(|m| d.app.i18n.msg(&m).to_string()).unwrap_or_default();
        let path = deep.join("themes").join("broken.toml").display().to_string();
        assert!(said.contains(&format!("{path}:3: #12345")), "{w} {lang:?}: {said}");
    }
    assert_eq!(std::fs::read_to_string("config.toml").unwrap(), CONFIG, "the config file is untouched");

    // A missing file is said too, by its path.
    std::fs::write("config.toml", "theme = \"gone\"\n").unwrap();
    let mut d = Demo::new(200, 24);
    let gone = slakio_core::config::Config {
        theme: "gone".to_string(),
        path: Some(Path::new("config.toml").to_path_buf()),
        ..Default::default()
    };
    d.app.load_at_start(TRUECOLOR, &gone, true, d.now);
    assert_eq!(d.app.theme.name, "terminal");
    assert!(
        d.status_line().contains("Theme not used: Unknown theme gone: there is no gone.toml"),
        "{}",
        d.status_line()
    );

    // While running: a good file is drawn and saved, a bad one is said in the palette.
    std::fs::write("themes/mine.toml", "extends = \"tokyo-night\"\n[colors]\nbg = \"#101820\"\n").unwrap();
    d.command("theme mine");
    assert_eq!((d.app.theme.name.as_ref(), d.app.theme.kind), ("mine", Kind::Truecolor));
    assert_eq!(d.app.theme.bg, Color::Rgb(0x10, 0x18, 0x20));
    assert_eq!(saved(&mut d), ["mine"]);
    assert_eq!(d.buffer()[(60, 5)].bg, Color::Rgb(0x10, 0x18, 0x20), "drawn on the file's background");
    d.command("set theme=broken");
    assert_eq!(d.app.theme.name, "mine", "nothing changes");
    assert!(saved(&mut d).is_empty());
    let why = d.app.palette_error(d.app.size).join(" ");
    assert!(why.starts_with(&native("Theme not used: themes/broken.toml:3: #12345 is not a colour")), "{why}");
    d.keys("esc");
    d.command("set theme=nord");
    d.command("set theme=mine");
    assert_eq!(d.app.theme.name, "mine");
    assert_eq!(saved(&mut d), ["nord", "mine"]);
    // A theme file named whole wins over a built-in theme its name matches in part (`drk`).
    std::fs::write("themes/drk.toml", "[colors]\naccent = \"red\"\n").unwrap();
    d.command("theme drk");
    assert_eq!((d.app.theme.name.as_ref(), d.app.theme.accent), ("drk", Color::Red));
    // A symbolic link is refused, and said.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("mine.toml", "themes/linked.toml").unwrap();
        d.command("theme linked");
        assert_eq!(d.app.theme.name, "drk");
        let why = d.app.palette_error(d.app.size).join(" ");
        assert_eq!(why, "Theme not used: themes/linked.toml is a symbolic link");
        d.keys("esc");
    }
    // `NO_COLOR` still draws without colour.
    d.app.look.no_color = true;
    d.command("theme mine");
    assert_eq!((d.app.theme.kind, d.app.theme_setting.as_str()), (Kind::NoColor, "mine"));

    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}
