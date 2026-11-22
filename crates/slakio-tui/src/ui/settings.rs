//! The settings screen over the dimmed screen: why a theme is not used, or why the folder or the
//! name of a theme copy typed cannot be (on top), every setting under its group as
//! `key ‹ value › what it means` (on a short screen the list scrolls and says how many lines are
//! out of view above and below), a rule, then what is typed and what the selected setting is for
//! and where it is saved.
//!
//! ```text
//! ╭ Settings ─────────────────────────────────────────────────────────────╮
//! │ Display                                                               │
//! │   Theme            ‹ auto        ›  Tokyo Night with 24-bit colour, … │
//! │   Density          ‹ comfortable ›  As in GUI Slack: an avatar block… │
//! │───────────────────────────────────────────────────────────────────────│
//! │ The colours. h/l show each theme at once; Enter keeps the one shown. │
//! │ Set with :set theme=…                                                 │
//! │ Saved to ~/.config/slakio/config.toml                                 │
//! ╰──────────────────────── j/k move · h/l change · Enter keep · Esc close ╯
//! ```
//!
//! The dimming skips the pictures' cells (their colour says which picture they are), so a picture
//! outside the box stays as it is; the box covers what is under it.

use super::modal;
use crate::app::App;
use crate::app::settings::Screen;
use crate::app::settings::table::{SETTINGS, Values, groups};
use crate::text::{clip, clip_middle, width};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use slakio_core::i18n::{Label, Msg};

/// The widest the box gets.
const MAX_W: u16 = 100;
/// The lines the list keeps on a short screen, before the lines under it get room.
const LIST_MIN: usize = 4;
/// At most this many lines say why a theme is not used.
const PROBLEM_LINES: usize = 3;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let Some(s) = app.prefs.screen.as_ref() else { return };
    let w = area.width.saturating_sub(4).min(MAX_W);
    let h = area.height.saturating_sub(2);
    if w < 20 || h < 6 {
        return;
    }
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let i18n = &app.i18n;
    // While a folder or a name is typed, the keys of what is typed.
    let typing = s.folder.is_some() || s.name.is_some();
    let keys = if typing { Label::SettingsInputKeys } else { Label::SettingsKeys };
    let inner = modal(f, app, rect, &i18n.label(Label::SettingsTitle), &i18n.label(keys));
    let tw = usize::from(inner.width).saturating_sub(2);
    let x = inner.x + 1;
    let bottom = inner.bottom();
    let mut y = inner.y;
    let put = |f: &mut Frame, y: u16, text: &str, style: Style| {
        f.buffer_mut().set_stringn(x, y, clip(text, tw), tw, style);
    };
    // Why the theme shown (or set) is not used, or the folder typed.
    let problem = s.preview.as_ref().and_then(|p| p.error.as_ref()).or(s.problem.as_ref());
    let typed_error = s.folder.as_ref().or(s.name.as_ref()).and_then(|(_, e)| e.as_ref());
    for msg in typed_error.into_iter().chain(problem) {
        for line in wrap(&i18n.msg(msg), tw).into_iter().take(PROBLEM_LINES) {
            if y < bottom {
                put(f, y, &line, t.warning());
                y += 1;
            }
        }
    }
    let foot = foot(app, s, tw);
    let room = usize::from(bottom.saturating_sub(y));
    let mut y = list(f, app, s, Rect { y, height: room as u16, ..inner }, foot.len());
    // The rule, unless the lines under it need its line.
    if usize::from(bottom.saturating_sub(y)) > foot.len() {
        let iw = usize::from(inner.width);
        f.buffer_mut().set_stringn(inner.x, y, "─".repeat(iw), iw, t.divider());
        y += 1;
    }
    for (i, (line, style)) in foot.iter().enumerate() {
        if y >= bottom {
            break;
        }
        put(f, y, line, *style);
        if i == 0 && typing {
            let cx = (x + width(line) as u16).min(x + tw as u16);
            f.set_cursor_position(Position { x: cx, y });
        }
        y += 1;
    }
}

/// The lines under the list: the folder typed, what the selected setting is for, where it is
/// saved.
fn foot(app: &App, s: &Screen, tw: usize) -> Vec<(String, Style)> {
    let t = &app.theme;
    let i18n = &app.i18n;
    let spec = &SETTINGS[s.selected.min(SETTINGS.len() - 1)];
    let mut foot: Vec<(String, Style)> = Vec::new();
    if let Some((text, _)) = &s.folder {
        foot.push((format!("{} {text}", i18n.label(Label::SettingsDownloadsPrompt)), t.text()));
    }
    if let Some((text, _)) = &s.name {
        foot.push((format!("{} {text}", i18n.label(Label::SettingsThemeNamePrompt)), t.text()));
        let msg = Msg::SettingsThemeNameHint { from: app.theme_shown() };
        foot.extend(wrap(&i18n.msg(&msg), tw).into_iter().map(|l| (l, t.warning())));
    }
    if let (Values::Themes, Some(p)) = (spec.values, &s.preview)
        && p.error.is_none()
        && s.name.is_none()
    {
        let name = s.themes.get(p.index).cloned().unwrap_or_default();
        let msg = Msg::SettingsThemePreview { name, current: app.theme_setting.clone() };
        foot.extend(wrap(&i18n.msg(&msg), tw).into_iter().map(|l| (l, t.warning())));
    }
    foot.extend(wrap(&i18n.label(spec.about), tw).into_iter().map(|l| (l, t.text())));
    // The row's name is a label: its key is said here, as `:set` takes it.
    let set_with = i18n.msg(&Msg::SettingsSetWith { key: spec.key.to_string() });
    foot.extend(wrap(&set_with, tw).into_iter().map(|l| (l, t.muted())));
    match app.config_file() {
        Some(path) if app.prefs.writable => {
            let path = path.display().to_string();
            foot.push((i18n.msg(&Msg::SettingsSaved { path: path.clone() }).to_string(), t.faint()));
            foot.push((i18n.msg(&Msg::SettingsMore { path }).to_string(), t.faint()));
        }
        Some(path) => {
            foot.push((i18n.label(Label::SettingsNotSaved).to_string(), t.warning()));
            foot.push((i18n.msg(&Msg::SettingsMore { path: path.display().to_string() }).to_string(), t.faint()));
        }
        None => foot.push((i18n.label(Label::SettingsNoFile).to_string(), t.warning())),
    }
    foot
}

/// The list in `area` (the box's inside under what is said on top), `foot` lines kept for what
/// is under it: the groups' headings and their rows, scrolled to keep the selected row in view.
/// Hands back the line under it.
fn list(f: &mut Frame, app: &App, s: &Screen, area: Rect, foot: usize) -> u16 {
    let t = &app.theme;
    let i18n = &app.i18n;
    let tw = usize::from(area.width).saturating_sub(2);
    let x = area.x + 1;
    let mut y = area.y;
    let room = usize::from(area.height);
    let put = |f: &mut Frame, y: u16, text: &str, style: Style| {
        f.buffer_mut().set_stringn(x, y, clip(text, tw), tw, style);
    };
    let groups = groups();
    let mut lines: Vec<Option<usize>> = Vec::new();
    for (_, rows) in &groups {
        lines.push(None);
        lines.extend(rows.iter().map(|&k| Some(k)));
    }
    let list_room = if lines.len() + 1 + foot <= room {
        lines.len()
    } else {
        room.saturating_sub(1 + foot).max(LIST_MIN.min(lines.len())).min(room)
    };
    // Scrolled, the list's first and last lines say how much is out of view, on lines of their
    // own (never over a row).
    let scrolls = lines.len() > list_room && list_room >= 3;
    let selected_line = lines.iter().position(|l| *l == Some(s.selected)).unwrap_or(0);
    let mut rows_room = if scrolls { list_room - 1 } else { list_room };
    let mut skip = (selected_line + 1).saturating_sub(rows_room);
    // The line on top is the count's only once rows are above.
    let top = scrolls && skip > 0;
    if top {
        rows_room -= 1;
        skip = (selected_line + 1).saturating_sub(rows_room);
    }
    let (above, below) = (skip, lines.len().saturating_sub(skip + rows_room));
    let key_w = SETTINGS.iter().map(|s| width(&i18n.label(s.label))).max().unwrap_or(0);
    let shown: Vec<(String, String)> = (0..SETTINGS.len()).map(|k| app.setting_shown(k)).collect();
    // A long folder is cut by its middle rather than widening the column (the default's width).
    let folder_w = width("~/Downloads/slakio");
    let value_w = (0..SETTINGS.len())
        .map(|k| if SETTINGS[k].values == Values::Folder { folder_w } else { width(&shown[k].0) })
        .max()
        .unwrap_or(0)
        .min(tw / 3);
    let list_top = y;
    y += u16::from(top);
    let mut group = groups.iter();
    for (i, line) in lines.iter().enumerate() {
        let heading = line.is_none().then(|| group.next()).flatten();
        if i < skip || i >= skip + rows_room {
            continue;
        }
        let row = Rect::new(area.x, y, area.width, 1);
        match (line, heading) {
            (None, Some((g, _))) => put(f, y, &i18n.label(g.label()), t.current()),
            (Some(k), _) => {
                draw_row(f, app, row, *k, &shown[*k], (key_w, value_w));
                if *k == s.selected {
                    super::highlight(f, app, row, Selection::Focused);
                }
            }
            _ => {}
        }
        y += 1;
    }
    // At the right end of the list's first and last lines.
    let list_bottom = list_top + list_room as u16;
    y = y.max(list_top + if scrolls { list_room as u16 } else { 0 });
    for (count, at, up) in [(above, list_top, true), (below, list_bottom.saturating_sub(1), false)] {
        if count == 0 || (up && list_room == 1 && below > 0) {
            continue;
        }
        let count = count as u64;
        let msg = if up { Msg::SettingsMoreAbove { count } } else { Msg::SettingsMoreBelow { count } };
        let text = format!(" {} ", i18n.msg(&msg));
        let end = x + tw as u16;
        let cx = end.saturating_sub(width(&text) as u16).max(x);
        f.buffer_mut().set_stringn(cx, at, &text, usize::from(end - cx), t.faint());
    }
    y
}

/// Setting `k`'s row, its value and what it means `shown`, its key and its value `widths` wide.
fn draw_row(f: &mut Frame, app: &App, row: Rect, k: usize, shown: &(String, String), widths: (usize, usize)) {
    let t = &app.theme;
    let (x, y) = (row.x + 1, row.y);
    let tw = usize::from(row.width).saturating_sub(2);
    let (value, what) = shown;
    let read_only = SETTINGS[k].values == Values::Folder && app.prefs.download_env.is_some();
    let value = if SETTINGS[k].values == Values::Folder { clip_middle(value, widths.1) } else { clip(value, widths.1) };
    let value = format!("‹ {} ›", pad(&value, widths.1));
    let text = format!("  {}  ", pad(&app.i18n.label(SETTINGS[k].label), widths.0));
    f.buffer_mut().set_stringn(x, y, clip(&text, tw), tw, t.text());
    let vx = x + width(&text) as u16;
    let left = tw.saturating_sub(width(&text));
    // Read-only, it stays readable: muted, as what it means.
    let style = if read_only { t.muted() } else { t.key_warm() };
    f.buffer_mut().set_stringn(vx, y, clip(&value, left), left, style);
    let wx = vx + width(&value) as u16 + 2;
    let rest = tw.saturating_sub(usize::from(wx.saturating_sub(x)));
    if rest > 0 {
        f.buffer_mut().set_stringn(wx, y, clip(what, rest), rest, t.muted());
    }
}

/// `text` padded with spaces to `w` columns.
fn pad(text: &str, w: usize) -> String {
    format!("{text}{}", " ".repeat(w.saturating_sub(width(text))))
}

/// `text` in lines of at most `w` columns, broken between words.
fn wrap(text: &str, w: usize) -> Vec<String> {
    crate::text::wrap(text, w, usize::MAX).0
}
