//! Themes while the app runs: `:theme <name>` (also `:colorscheme`, `:colo` and `:set theme=`)
//! draws with the theme at once and asks for it to be saved in the config file; a family takes
//! the variant of the terminal's background; `NO_COLOR` keeps the screen without color; a name
//! the setting does not take changes nothing and the palette says which names there are.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use slakio_tui::app::Effect;
use slakio_tui::theme::{Background, Look, NAMES};

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

#[test]
fn colon_theme_switches_at_once_and_asks_to_save_it() {
    let mut d = Demo::new(120, 40);
    assert_eq!(d.app.theme.name, "terminal");
    d.command("theme nord");
    assert_eq!((d.app.theme.name, d.app.theme_setting.as_str()), ("nord", "nord"));
    assert_eq!(saved(&mut d), ["nord"]);
    assert!(d.status_line().contains("Theme: nord (saved)"), "{}", d.status_line());
    d.command("colo dracula");
    d.command("set theme=gruvbox-light");
    d.command("colorscheme catppuccin-mocha");
    assert_eq!(d.app.theme.name, "catppuccin-mocha");
    assert_eq!(saved(&mut d), ["dracula", "gruvbox-light", "catppuccin-mocha"]);
}

#[test]
fn every_name_the_setting_takes_can_be_picked() {
    let mut d = Demo::new(80, 24);
    for name in NAMES {
        d.command(&format!("theme {name}"));
        assert_eq!(d.app.theme_setting, *name);
        let _ = d.screen();
    }
    assert_eq!(saved(&mut d).len(), NAMES.len());
}

#[test]
fn a_family_follows_the_background_and_no_color_stays_without_color() {
    let mut d = Demo::new(120, 40);
    d.app.look = Look { truecolor: true, background: Background::Light, no_color: false };
    d.command("theme tokyo-night");
    assert_eq!(d.app.theme.name, "tokyo-night-day");
    d.command("theme auto");
    assert_eq!(d.app.theme.name, "tokyo-night-day", "auto on a truecolor terminal");
    d.app.look.no_color = true;
    d.command("theme dracula");
    assert_eq!(d.app.theme.name, "no-color", "NO_COLOR wins");
    assert_eq!(d.app.theme_setting, "dracula", "the setting is still taken and saved");
}

#[test]
fn an_unknown_theme_changes_nothing_and_names_the_ones_there_are() {
    let mut d = Demo::new(160, 40);
    d.command("theme nord");
    let _ = saved(&mut d);
    d.command("theme solarized");
    assert_eq!(d.app.theme.name, "nord");
    assert!(saved(&mut d).is_empty(), "nothing saved");
    let why = d.app.palette_error(d.app.size).join(" ");
    assert!(why.starts_with("Unknown theme solarized (use auto, terminal") && why.contains("dracula"), "{why}");
}
