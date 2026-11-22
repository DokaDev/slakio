use super::{MAX_W, columns, what_room};
use crate::app::App;
use crate::app::settings::Screen;
use crate::app::settings::table::{SETTINGS, Values};
use crate::text::width;
use crate::theme::{Background, Look, Theme};
use slakio_core::config::{self, Config};
use slakio_core::i18n::{Lang, Msg};
use std::time::Instant;

/// The width of the list's rows on a screen `cols` wide: the box, its borders, a margin each side.
fn row_width(cols: u16) -> usize {
    usize::from(cols.saturating_sub(4).min(MAX_W)).saturating_sub(2 + 2)
}

fn app(lang: Lang) -> App {
    let mut a = App::new(lang, Theme::terminal());
    let look = Look { truecolor: true, background: Background::Dark, no_color: false };
    a.load_at_start(look, &Config::default(), true, Instant::now());
    a.prefs.download_env = None;
    a.prefs.screen = Some(Screen::default());
    a
}

/// Every state a row can be in, one after another: every built-in theme and a theme file (with
/// and without an error), every fixed value, every preset size and the widest and narrowest
/// size `:set` takes, the folder set by the environment, a config file that cannot be saved.
/// Hands back, for each state, every row's value and what it means.
fn states(a: &mut App) -> Vec<Vec<(String, String)>> {
    let now = Instant::now();
    let mut out = Vec::new();
    let shown = |a: &App| (0..SETTINGS.len()).map(|k| a.setting_shown(k)).collect::<Vec<_>>();
    for (k, spec) in SETTINGS.iter().enumerate() {
        match spec.values {
            Values::Themes => {
                for name in config::THEMES.iter().map(|t| t.to_string()).chain(["my-theme".to_string()]) {
                    a.theme_setting = name;
                    for problem in [None, Some(Msg::SetNoValue { key: "theme".into() })] {
                        a.prefs.screen.as_mut().unwrap().problem = problem;
                        out.push(shown(a));
                    }
                }
                a.theme_setting = "auto".into();
                a.prefs.screen.as_mut().unwrap().problem = None;
            }
            Values::Fixed(values) => {
                for (v, _) in values {
                    a.apply_setting(k, v, now).unwrap();
                    out.push(shown(a));
                }
            }
            Values::Megabytes(presets) => {
                let (lo, hi) = (*config::DOWNLOAD_MAX_MB.start(), *config::DOWNLOAD_MAX_MB.end());
                for mb in presets.iter().copied().chain([lo, hi]) {
                    a.apply_setting(k, &mb.to_string(), now).unwrap();
                    out.push(shown(a));
                }
            }
            Values::Folder => {
                a.prefs.download_env = Some("/somewhere/else".into());
                out.push(shown(a));
                a.prefs.download_env = None;
                out.push(shown(a));
            }
        }
    }
    a.prefs.writable = false;
    out.push(shown(a));
    a.prefs.writable = true;
    out
}

/// What every value means fits beside it on an 80-column screen, in every language (display
/// width: a Hangul syllable takes two cells), in every state the rows can be in: never cut.
#[test]
fn every_value_s_meaning_fits_at_80_columns_in_every_language() {
    let tw = row_width(80);
    for lang in [Lang::En, Lang::Ko] {
        let mut a = app(lang);
        let states = states(&mut a);
        assert!(states.len() > 40, "{lang:?}: every value walked");
        for shown in &states {
            let room = what_room(tw, columns(&a, shown, tw));
            for (k, (value, what)) in shown.iter().enumerate() {
                assert!(
                    width(what) <= room,
                    "{lang:?}: {} = {value}: {what:?} is {} columns, {room} fit",
                    SETTINGS[k].key,
                    width(what)
                );
            }
        }
    }
}
