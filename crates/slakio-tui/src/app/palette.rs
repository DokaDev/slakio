//! The command palette: the `:` command line (also `Ctrl+P`) with the entries the text matches
//! listed under it, generated from the action registry with the keys of each. With nothing
//! typed it lists every command and every action that works where the keyboard is; a word
//! lists the commands it starts (an exact name first), then the actions its letters find
//! ([`crate::action::search`]); `:theme ` lists the themes; `:rename <name>` names the tab shown. `Enter` runs the selected entry (a
//! command that needs its argument is completed into the input instead); an empty line, or
//! text that matches nothing, runs nothing: the first closes, the second says why under the
//! list and the palette stays.
//!
//! ```text
//! ╭ Commands ──────────────────────────────────────────────────────╮
//! │ : type a command or an action name                              │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  :qa, :qall, :quitall  Quit                    Ctrl+Q / Space q │
//! │  :theme <theme>        Change the color theme                   │
//! │                        Next panel                      Tab / F6 │
//! ╰───────────────────────────── Tab/↑↓ select · Enter run · Esc close ╯
//! ```

use super::{App, Effect};
use crate::action::TabAction;
use crate::action::{self, Action, AppAction, HelpAction, REGISTRY};
use crate::keymap::keys;
use crate::screen::{self, PaletteBox};
use crate::text::wrap;
use crate::theme::{self, NAMES};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use slakio_core::i18n::{Label, Msg};
use std::time::Instant;

/// The names `:theme` goes by (vim's `:colorscheme` too). `:set theme=<name>` works as well.
const THEME_COMMANDS: &[&str] = &["theme", "colorscheme", "colo"];

const HELP: Action = Action::Help(HelpAction::Open);

/// At most this many of a command's names are shown (all of them are typed).
const NAMES_SHOWN: usize = 3;

/// How an entry sorts: tier, `Quit`, minus the score, an action (not a command), place.
type Order = (u8, bool, i32, bool, usize, u8);

/// One entry of the palette's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// A `:` command of the registry (index into [`REGISTRY`]).
    Command(usize),
    /// `:theme`, which takes the name of a theme.
    ThemeCommand,
    /// A theme for `:theme` (index into [`NAMES`]).
    Theme(usize),
    /// A value for `:avatars <value>` (index into [`AVATAR_VALUES`]).
    Avatars(usize),
    /// `:rename <name>`: name the tab shown that (nothing: after what it shows).
    Rename,
    /// A value for `:icons <value>` (index into [`ICON_VALUES`]).
    Icons(usize),
    /// A value for `:density <value>` (index into [`DENSITY_VALUES`]).
    Density(usize),
    /// An action found by its words (index into [`REGISTRY`]).
    Action(usize),
}

/// An entry ready to draw: what it completes to (empty for an action), what it does, and its
/// keys where the keyboard is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub label: String,
    pub keys: String,
}

/// Actions the palette offers: not the keys of another popup, of typing, or of the palette
/// itself; the shell's and the panes' only with a backend.
fn offered(a: Action, backend: bool) -> bool {
    match a {
        Action::App(AppAction::Quit) => true,
        Action::App(
            AppAction::ChooseWorkspace | AppAction::ToggleAvatars | AppAction::ToggleIcons | AppAction::ToggleDensity,
        )
        | Action::Shell(_)
        | Action::Pane(_)
        | Action::Tab(_) => backend,
        Action::Help(h) => h == HelpAction::Open,
        Action::App(_) | Action::CommandLine(_) | Action::Composer(_) | Action::Dialog(_) => false,
    }
}

/// The themes `arg` names, best first ([`action::rank`]); ties keep the themes' order.
fn themes(arg: &str) -> Vec<usize> {
    let mut hits: Vec<(u8, i32, usize)> =
        NAMES.iter().enumerate().filter_map(|(i, n)| action::rank(arg, &[], &[n]).map(|(t, s)| (t, -s, i))).collect();
    hits.sort_unstable();
    hits.into_iter().map(|(.., i)| i).collect()
}

/// What a theme name stands for.
fn theme_label(name: &str) -> Label {
    match name {
        "terminal" => Label::ThemesTerminal,
        "dark" => Label::ThemesDark,
        "light" => Label::ThemesLight,
        "high-contrast" => Label::ThemesHighContrast,
        "catppuccin" => Label::ThemesCatppuccin,
        "catppuccin-latte" => Label::ThemesCatppuccinLatte,
        "catppuccin-mocha" => Label::ThemesCatppuccinMocha,
        "tokyo-night" => Label::ThemesTokyoNight,
        "tokyo-night-day" => Label::ThemesTokyoNightDay,
        "tokyo-night-night" => Label::ThemesTokyoNightNight,
        "gruvbox" => Label::ThemesGruvbox,
        "gruvbox-light" => Label::ThemesGruvboxLight,
        "gruvbox-dark" => Label::ThemesGruvboxDark,
        "nord" => Label::ThemesNord,
        "dracula" => Label::ThemesDracula,
        _ => Label::ThemesAuto,
    }
}

/// The name of `:rename <name>` (empty: `:rename ` with nothing after it).
fn rename_arg(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("rename")?;
    rest.starts_with(char::is_whitespace).then(|| rest.trim())
}

impl App {
    /// The palette's entries for the text typed so far: every one [`action::rank`] matches,
    /// the better match first; on a tie, commands in the registry's order (`:theme` after
    /// the help), then the actions. `Quit` comes after everything that matches as well, so with
    /// nothing typed it is last and never the entry `Enter` or a stray click would run.
    pub fn palette_items(&self) -> Vec<Item> {
        let line = self.cmdline.text().trim_start();
        if rename_arg(line).is_some() && self.backend.is_some() {
            return vec![Item::Rename];
        }
        if let Some(arg) = theme_arg(line) {
            return themes(arg).into_iter().map(Item::Theme).collect();
        }
        if let Some(arg) = setting_arg(line, "density") {
            return ranked(DENSITY_VALUES, arg).into_iter().map(Item::Density).collect();
        }
        if let Some(arg) = icons_arg(line) {
            let mut hits: Vec<(u8, i32, usize)> = ICON_VALUES
                .iter()
                .enumerate()
                .filter_map(|(i, v)| action::rank(arg, &[], &[v]).map(|(t, s)| (t, -s, i)))
                .collect();
            hits.sort_unstable();
            return hits.into_iter().map(|(.., i)| Item::Icons(i)).collect();
        }
        if let Some(arg) = avatars_arg(line) {
            let mut hits: Vec<(u8, i32, usize)> = AVATAR_VALUES
                .iter()
                .enumerate()
                .filter_map(|(i, v)| action::rank(arg, &[], &[v]).map(|(t, s)| (t, -s, i)))
                .collect();
            hits.sort_unstable();
            return hits.into_iter().map(|(.., i)| Item::Avatars(i)).collect();
        }
        let word = line.trim_end();
        if word.contains(char::is_whitespace) {
            return Vec::new();
        }
        let backend = self.backend.is_some();
        let below = self.region_context();
        let english = slakio_core::i18n::I18n::new(slakio_core::i18n::Lang::En);
        let mut hits: Vec<(Order, Item)> = Vec::new();
        for (i, s) in REGISTRY.iter().enumerate() {
            // What works here: a command, or a key from where the keyboard is.
            let works = offered(s.action, backend)
                && (!s.commands.is_empty() || !self.keymap.keys_for(s.action, below).is_empty());
            if works && let Some((tier, score)) = action::rank_spec(word, s, &self.i18n) {
                let quit = s.action == Action::App(AppAction::Quit);
                let item = if s.commands.is_empty() { Item::Action(i) } else { Item::Command(i) };
                hits.push(((tier, quit, -score, s.commands.is_empty(), i, 0), item));
            }
            if s.action == HELP {
                let (own, en) = (self.i18n.label(Label::ActionTheme), english.label(Label::ActionTheme));
                if let Some((tier, score)) = action::rank(word, THEME_COMMANDS, &[&own, &en]) {
                    hits.push(((tier, false, -score, false, i, 1), Item::ThemeCommand));
                }
            }
        }
        hits.sort_unstable_by_key(|h| h.0);
        hits.into_iter().map(|(_, item)| item).collect()
    }

    /// The entries as drawn.
    pub fn palette_rows(&self) -> Vec<Row> {
        let below = self.region_context();
        let keys_of = |a: Action| {
            let mut all = self.keymap.keys_for(a, below);
            all.sort_by_key(Vec::len);
            all.dedup();
            all.iter().take(2).map(|k| keys::label(k)).collect::<Vec<_>>().join(" / ")
        };
        let label = |l: Label| self.i18n.label(l).to_string();
        self.palette_items()
            .into_iter()
            .map(|item| match item {
                Item::Command(i) => Row {
                    name: REGISTRY[i]
                        .commands
                        .iter()
                        .take(NAMES_SHOWN)
                        .map(|c| format!(":{c}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    label: label(REGISTRY[i].label),
                    keys: keys_of(REGISTRY[i].action),
                },
                Item::ThemeCommand => Row {
                    name: format!(":theme {}", label(Label::PaletteThemeArg)),
                    label: label(Label::ActionTheme),
                    keys: String::new(),
                },
                Item::Theme(i) => Row {
                    name: NAMES[i].to_string(),
                    label: label(theme_label(NAMES[i])),
                    keys: if NAMES[i] == self.theme_setting { label(Label::PaletteCurrent) } else { String::new() },
                },
                Item::Avatars(i) => {
                    let on = AVATAR_VALUES[i] == "initials";
                    Row {
                        name: AVATAR_VALUES[i].to_string(),
                        label: label(if on { Label::AvatarsInitials } else { Label::AvatarsOff }),
                        keys: if on == self.settings.avatars { label(Label::PaletteCurrent) } else { String::new() },
                    }
                }
                Item::Action(i) => {
                    Row { name: String::new(), label: label(REGISTRY[i].label), keys: keys_of(REGISTRY[i].action) }
                }
                Item::Icons(i) => {
                    let on = ICON_VALUES[i] == "on";
                    Row {
                        name: ICON_VALUES[i].to_string(),
                        label: label(if on { Label::IconsOn } else { Label::IconsOff }),
                        keys: if on == self.settings.icons { label(Label::PaletteCurrent) } else { String::new() },
                    }
                }
                Item::Density(i) => {
                    let compact = DENSITY_VALUES[i] == "compact";
                    Row {
                        name: DENSITY_VALUES[i].to_string(),
                        label: label(if compact { Label::DensityCompact } else { Label::DensityComfortable }),
                        keys: if compact == self.settings.compact {
                            label(Label::PaletteCurrent)
                        } else {
                            String::new()
                        },
                    }
                }
                Item::Rename => {
                    let name = rename_arg(self.cmdline.text()).unwrap_or_default().to_string();
                    let what = if name.is_empty() {
                        label(Label::PaletteRenameAuto)
                    } else {
                        self.i18n.msg(&Msg::PaletteRenameTo { name: name.clone() }).to_string()
                    };
                    let arg = if name.is_empty() { label(Label::PaletteRenameArg) } else { name };
                    Row { name: format!(":rename {arg}"), label: what, keys: keys_of(Action::Tab(TabAction::Rename)) }
                }
            })
            .collect()
    }

    /// Why the last `Enter` ran nothing, wrapped to the width of the palette on a screen `size`
    /// (four lines at most).
    pub fn palette_error(&self, size: Rect) -> Vec<String> {
        let Some(e) = &self.cmdline.error else { return Vec::new() };
        let w = usize::from(screen::palette_width(size.width)).saturating_sub(6);
        wrap(self.i18n.msg(e).as_ref(), w, 4).0
    }

    /// Where the palette is drawn now (`None` when closed or the screen is too small).
    pub fn palette_box(&self) -> Option<PaletteBox> {
        self.palette_box_in(self.size)
    }

    /// Where the palette is drawn on a screen `size`.
    pub fn palette_box_in(&self, size: Rect) -> Option<PaletteBox> {
        if !self.cmdline.is_open() {
            return None;
        }
        let n = self.palette_items().len();
        screen::palette(size, n, self.cmdline.selected, self.palette_error(size).len() as u16)
    }

    /// Move the selection by `by` entries.
    pub(super) fn palette_step(&mut self, by: isize) {
        let n = self.palette_items().len();
        self.cmdline.step(by, n);
    }

    /// `Enter`: run the selected entry (see the module's text).
    pub(super) fn palette_run(&mut self, now: Instant) {
        let text = self.cmdline.text().trim().to_string();
        if text.is_empty() && !self.cmdline.picked {
            return self.cmdline.close();
        }
        match self.palette_items().get(self.cmdline.selected).copied() {
            Some(Item::Command(i) | Item::Action(i)) => {
                self.cmdline.close();
                self.dispatch(REGISTRY[i].action, now);
            }
            Some(Item::ThemeCommand) => self.cmdline.set("theme "),
            Some(Item::Theme(i)) => {
                self.cmdline.close();
                if let Err(msg) = self.set_theme(NAMES[i], now) {
                    self.warn(msg, now);
                }
            }
            Some(Item::Avatars(i)) => {
                self.cmdline.close();
                if let Err(msg) = self.set_avatars(AVATAR_VALUES[i], now) {
                    self.warn(msg, now);
                }
            }
            Some(Item::Icons(i)) => {
                self.cmdline.close();
                if let Err(msg) = self.set_icons(ICON_VALUES[i], now) {
                    self.warn(msg, now);
                }
            }
            Some(Item::Density(i)) => {
                self.cmdline.close();
                if let Err(msg) = self.set_density(DENSITY_VALUES[i], now) {
                    self.warn(msg, now);
                }
            }
            Some(Item::Rename) => {
                let name = rename_arg(&text).unwrap_or_default().to_string();
                self.cmdline.close();
                if !self.work.rename(&name) {
                    self.info(Msg::Label(Label::StatusNoTab), now);
                }
            }
            None => {
                let error = match (theme_arg(&text), avatars_arg(&text), icons_arg(&text)) {
                    (Some(name), ..) => Msg::ThemeUnknown { name: name.to_string(), names: NAMES.join(", ") },
                    (_, Some(name), _) => {
                        Msg::AvatarsUnknown { name: name.to_string(), names: AVATAR_VALUES.join(", ") }
                    }
                    (.., Some(name)) => Msg::IconsUnknown { name: name.to_string(), names: ICON_VALUES.join(", ") },
                    _ if setting_arg(&text, "density").is_some() => Msg::DensityUnknown {
                        name: setting_arg(&text, "density").unwrap_or_default().to_string(),
                        names: DENSITY_VALUES.join(", "),
                    },
                    _ => {
                        let word = text.split_whitespace().next().unwrap_or_default().to_string();
                        if action::by_command(&word).is_some() {
                            Msg::CommandNoArgs { name: word }
                        } else {
                            Msg::CommandUnknown { name: text }
                        }
                    }
                };
                self.cmdline.error = Some(error);
            }
        }
    }

    /// The mouse over the palette: the wheel moves the selection, a click on an entry runs it
    /// (`Quit` asks first), a click outside closes the palette. `true` when the screen changed.
    pub(super) fn palette_mouse(&mut self, m: MouseEvent, now: Instant) -> bool {
        let Some(b) = self.palette_box() else { return false };
        let at = Position { x: m.column, y: m.row };
        match m.kind {
            MouseEventKind::ScrollDown => self.palette_step(1),
            MouseEventKind::ScrollUp => self.palette_step(-1),
            MouseEventKind::Down(MouseButton::Left) if b.list.contains(at) => {
                let i = b.first + usize::from(at.y - b.list.y);
                if i >= self.palette_items().len() {
                    return false;
                }
                self.cmdline.selected = i;
                self.cmdline.picked = true;
                let quit = REGISTRY.iter().position(|s| s.action == Action::App(AppAction::Quit));
                if matches!(self.palette_items()[i], Item::Command(q) | Item::Action(q) if Some(q) == quit) {
                    // One click is easy to make by mistake: quitting asks first.
                    self.cmdline.close();
                    self.confirm_quit();
                } else {
                    self.palette_run(now);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if !b.rect.contains(at) => self.cmdline.close(),
            _ => return false,
        }
        true
    }
}

/// The values `:density` takes.
pub const DENSITY_VALUES: &[&str] = &["comfortable", "compact"];

/// The value of `:<key> <value>` (also `:set <key>=<value>`).
fn setting_arg<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let line = line.trim_start();
    let (word, rest) = line.split_once(char::is_whitespace)?;
    match word {
        w if w == key => Some(rest.trim()),
        "set" => rest.trim().strip_prefix(key)?.strip_prefix('=').map(str::trim),
        _ => None,
    }
}

/// The indices of `values` that `arg` matches, best first.
fn ranked(values: &[&str], arg: &str) -> Vec<usize> {
    let mut hits: Vec<(u8, i32, usize)> =
        values.iter().enumerate().filter_map(|(i, v)| action::rank(arg, &[], &[v]).map(|(t, s)| (t, -s, i))).collect();
    hits.sort_unstable();
    hits.into_iter().map(|(.., i)| i).collect()
}

/// The values `:icons` takes (`icons = "ask"` stays for the first run).
pub const ICON_VALUES: &[&str] = &["on", "off"];

/// The value of `:icons <value>` (also `:set icons=<value>`).
pub fn icons_arg(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let (word, rest) = line.split_once(char::is_whitespace)?;
    match word {
        "icons" => Some(rest.trim()),
        "set" => rest.trim().strip_prefix("icons=").map(str::trim),
        _ => None,
    }
}

impl App {
    /// `:theme <name>`: draw with theme `name` from now on and save it in the config file. A
    /// name the setting does not take changes nothing and says which ones it takes.
    pub fn set_theme(&mut self, name: &str, now: Instant) -> Result<(), Msg> {
        let name = name.trim().to_ascii_lowercase();
        if !theme::NAMES.contains(&name.as_str()) {
            return Err(Msg::ThemeUnknown { name, names: theme::NAMES.join(", ") });
        }
        self.theme = self.look.theme(&name);
        self.theme_setting.clone_from(&name);
        self.effects.push(Effect::Save { key: "theme", value: name.clone() });
        self.info(Msg::ThemeChanged { name }, now);
        Ok(())
    }

    /// `:avatars <initials|off>`: picture people by their initials chip, or not, from now on,
    /// and save it in the config file. Another value changes nothing and says which ones work.
    pub fn set_avatars(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        if !AVATAR_VALUES.contains(&value.as_str()) {
            return Err(Msg::AvatarsUnknown { name: value, names: AVATAR_VALUES.join(", ") });
        }
        self.settings.avatars = value == "initials";
        self.effects.push(Effect::Save { key: "avatars", value: value.clone() });
        self.info(Msg::AvatarsChanged { name: value }, now);
        Ok(())
    }

    /// `:avatars`, `:density`, `:icons` alone: the other value of the setting, saved.
    pub(super) fn toggle(&mut self, a: AppAction, now: Instant) {
        let s = self.settings;
        let done = match a {
            AppAction::ToggleAvatars => self.set_avatars(if s.avatars { "off" } else { "initials" }, now),
            AppAction::ToggleDensity => self.set_density(if s.compact { "comfortable" } else { "compact" }, now),
            _ => self.set_icons(if s.icons { "off" } else { "on" }, now),
        };
        if let Err(msg) = done {
            self.warn(msg, now);
        }
    }

    /// `:density <comfortable|compact>`: lay messages out so from now on, and save it in the
    /// config file. Another value changes nothing and says which ones work.
    pub fn set_density(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        if !DENSITY_VALUES.contains(&value.as_str()) {
            return Err(Msg::DensityUnknown { name: value, names: DENSITY_VALUES.join(", ") });
        }
        self.settings.compact = value == "compact";
        self.effects.push(Effect::Save { key: "density", value: value.clone() });
        self.info(Msg::DensityChanged { name: value }, now);
        Ok(())
    }

    /// `:icons <on|off>`: draw Nerd Font icons, or text instead, from now on, and save it in the
    /// config file. Another value changes nothing and says which ones work.
    pub fn set_icons(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        if !ICON_VALUES.contains(&value.as_str()) {
            return Err(Msg::IconsUnknown { name: value, names: ICON_VALUES.join(", ") });
        }
        self.settings.icons = value == "on";
        self.effects.push(Effect::Save { key: "icons", value: value.clone() });
        self.info(Msg::IconsChanged { name: value }, now);
        Ok(())
    }
}

/// The values `:avatars` takes (the config file also keeps `image` for photos, later).
pub const AVATAR_VALUES: &[&str] = &["initials", "off"];

/// The value of `:avatars <value>` (also `:set avatars=<value>`).
pub fn avatars_arg(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let (word, rest) = line.split_once(char::is_whitespace)?;
    match word {
        "avatars" => Some(rest.trim()),
        "set" => rest.trim().strip_prefix("avatars=").map(str::trim),
        _ => None,
    }
}

/// The theme name of `:theme <name>` (also `:colorscheme`, `:colo` as in vim, and
/// `:set theme=<name>`).
pub fn theme_arg(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let (word, rest) = line.split_once(char::is_whitespace)?;
    match word {
        "theme" | "colorscheme" | "colo" => Some(rest.trim()),
        "set" => rest.trim().strip_prefix("theme=").map(str::trim),
        _ => None,
    }
}
