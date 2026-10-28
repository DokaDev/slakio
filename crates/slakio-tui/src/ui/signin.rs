//! The sign-in screen: how to copy a request from the browser, what happens to it, and where
//! signing in stands (checking, or why the last paste did not sign in). Then the keys to quit
//! and to open the help.

use super::empty::Content;
use crate::app::App;
use crate::app::signin::{Picker, SignIn};
use crate::text::{clip, width, wrap};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};

/// The widest the text runs, for lines that read well.
const WIDTH: u16 = 76;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect, state: SignIn) {
    match state {
        SignIn::Desktop(p) => draw_picker(f, app, area, &p, false),
        SignIn::DesktopChecking(p) => draw_picker(f, app, area, &p, true),
        other => draw_paste(f, app, area, other),
    }
}

/// The desktop-workspace picker: the workspaces the Slack app is signed in to, one row each (a
/// check when marked, the name, and the host muted on the right) with the app's selection bar; the keychain-prompt
/// advice on a line of its own (also while checking and after a failure); then where it stands
/// and the keys, and the explanation.
fn draw_picker(f: &mut Frame, app: &App, area: Rect, p: &Picker, checking: bool) {
    let t = &app.theme;
    let w = area.width.saturating_sub(4).min(WIDTH);
    let text = |l: Label| app.i18n.label(l).to_string();
    let lines = |l: Label, max: usize, style| -> Vec<Line<'static>> {
        wrap(&text(l), usize::from(w), max).0.into_iter().map(|s| Line::styled(s, style)).collect()
    };
    let mut top = vec![Line::styled(text(Label::SigninDesktopTitle), t.current()), Line::default()];
    top.extend(beside(app, usize::from(w)));
    top.extend(lines(Label::SigninDesktopFound, 2, t.text()));
    top.push(Line::default());
    let mut bottom = vec![Line::default()];
    bottom.extend(lines(Label::SigninDesktopAllow, 2, t.warning()));
    bottom.push(Line::default());
    if checking {
        bottom.extend(lines(Label::SigninDesktopReading, 2, t.accent()));
    } else {
        if let Some(note) = p.note {
            bottom.extend(lines(note, 3, t.warning()));
        }
        bottom.extend(lines(Label::SigninDesktopKeys, 3, t.faint()));
        bottom.push(Line::default());
        bottom.extend(lines(Label::SigninDesktopNote, 4, t.muted()));
    }
    // As many rows as fit, the cursor's always among them; when not all fit, a muted line on the
    // blank line above and the one below says how many more there are that way, and how many of
    // them are marked (the rows keep all their room).
    let fixed = top.len() + bottom.len() + 4;
    let room = usize::from(area.height).saturating_sub(fixed).max(1);
    let scrolled = p.rows.len() > room;
    let n = p.rows.len().min(room);
    let first = (p.selected + 1).saturating_sub(n).min(p.rows.len() - n);
    let (above, below) = (first, p.rows.len() - first - n);
    let rows = |f: &mut Frame, at: Rect| {
        let mark = |f: &mut Frame, y: u16, arrow: &str, hidden: std::ops::Range<usize>| {
            let (more, marked) = (hidden.len(), hidden.filter(|&i| p.is_marked(i)).count());
            if more > 0 && y >= area.y && y < area.bottom() {
                let count = more as u64;
                let msg = if marked > 0 {
                    Msg::SigninDesktopMoreMarked { count, marked: marked.to_string() }
                } else {
                    Msg::SigninDesktopMore { count }
                };
                let text = format!("{arrow} {}", app.i18n.msg(&msg));
                f.buffer_mut().set_string(at.x + 1, y, clip(&text, usize::from(at.width).saturating_sub(2)), t.muted());
            }
        };
        if scrolled && at.height > 0 {
            mark(f, at.y.saturating_sub(1), "↑", 0..above);
            mark(f, at.y + n as u16, "↓", first + n..first + n + below);
        }
        for (i, row) in p.rows.iter().enumerate().skip(first).take(n) {
            let y = at.y + (i - first) as u16;
            if y >= at.bottom() {
                break;
            }
            let picked = i == p.selected;
            let host_w = width(&row.host).min(usize::from(at.width) / 2);
            let room = usize::from(at.width).saturating_sub(host_w + 6);
            let buf = f.buffer_mut();
            // A marked workspace (signed in to with the others marked) has a check before it.
            if p.is_marked(i) {
                buf.set_string(at.x + 1, y, "✓", t.accent());
            }
            buf.set_string(at.x + 3, y, clip(&row.name, room), if picked { t.bold() } else { t.text() });
            let hx = at.right().saturating_sub(host_w as u16 + 1);
            buf.set_string(hx, y, clip(&row.host, host_w), t.muted());
            if picked {
                let how = if checking { Selection::Unfocused } else { Selection::Focused };
                t.paint_selection(f.buffer_mut(), Rect { y, height: 1, ..at }, how);
            }
        }
    };
    render_parts(f, app, area, w, top, n as u16, rows, bottom);
}

/// Center `lines` in `area` with the quit/help keys below, the common frame of the sign-in
/// screens.
fn render_centered(f: &mut Frame, app: &App, area: Rect, w: u16, lines: Vec<Line<'static>>) {
    render_parts(f, app, area, w, lines, 0, |_, _| {}, Vec::new());
}

/// Center `top`, a block of `mid_h` rows drawn by `mid`, and `bottom` in `area`, `w` wide, with
/// the quit/help keys below.
#[expect(clippy::too_many_arguments, reason = "the parts of one centred block")]
fn render_parts(
    f: &mut Frame,
    app: &App,
    area: Rect,
    w: u16,
    top: Vec<Line<'static>>,
    mid_h: u16,
    mid: impl FnOnce(&mut Frame, Rect),
    bottom: Vec<Line<'static>>,
) {
    use crate::action::{Action, AppAction, HelpAction};
    use crate::keymap::Ctx;
    let keys = Content::keys(
        app,
        Ctx::Root,
        &[(Action::App(AppAction::Quit), Label::EmptyQuit), (Action::Help(HelpAction::Open), Label::EmptyHelp)],
    );
    let keys = Content { headline: None, sentence: None, keys, tip: None };
    let (top_h, bottom_h) = (top.len() as u16, bottom.len() as u16);
    let h = top_h + mid_h + bottom_h;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h + 4) / 2;
    let at = Rect::new(x, y, w, h.min(area.height)).intersection(area);
    f.render_widget(Paragraph::new(top), Rect { height: top_h, ..at }.intersection(at));
    let low = Rect { y: at.y + top_h + mid_h, height: bottom_h, ..at }.intersection(at);
    f.render_widget(Paragraph::new(bottom), low);
    // Last: the middle may draw on the blank lines that end `top` and begin `bottom`.
    mid(f, Rect { y: at.y + top_h, height: mid_h, ..at }.intersection(at));
    let below = Rect::new(x, at.bottom().saturating_add(1), w, 3).intersection(area);
    keys.draw_top(f, app, Rect { x: below.x.saturating_sub(1), width: below.width + 2, ..below });
}

fn draw_paste(f: &mut Frame, app: &App, area: Rect, state: SignIn) {
    let t = &app.theme;
    let w = area.width.saturating_sub(4).min(WIDTH);
    let text = |l: Label| app.i18n.label(l).to_string();
    let mut lines: Vec<Line<'static>> = vec![Line::styled(text(Label::SigninTitle), t.current()), Line::default()];
    lines.extend(beside(app, usize::from(w)));
    for step in [Label::SigninStep1, Label::SigninStep2, Label::SigninStep3, Label::SigninStep4] {
        lines.extend(step_lines(&text(step), usize::from(w)).into_iter().map(|l| Line::styled(l, t.text())));
    }
    lines.push(Line::default());
    lines.extend(wrap(&text(Label::SigninNote), usize::from(w), 4).0.into_iter().map(|l| Line::styled(l, t.muted())));
    let (said, style) = match state {
        SignIn::Checking => (Label::SigninChecking, t.accent()),
        SignIn::Failed(why) => (why, t.warning()),
        // Desktop states are drawn by `draw_picker`; the paste screen otherwise waits.
        _ => (Label::SigninWaiting, t.faint()),
    };
    lines.push(Line::default());
    lines.extend(wrap(&text(said), usize::from(w), 3).0.into_iter().map(|line| Line::styled(line, style)));
    render_centered(f, app, area, w, lines);
}

/// Signed in to other workspaces already: a muted line (`w` wide) that says they stay and this
/// adds one, then a blank line; nothing at the first sign-in.
fn beside(app: &App, w: usize) -> Vec<Line<'static>> {
    let already = app.signed_in_already();
    if already.is_empty() {
        return Vec::new();
    }
    let said = app.i18n.msg(&Msg::SigninAdds { workspace: already.join(", ") }).to_string();
    let mut lines: Vec<Line<'static>> =
        wrap(&said, w, 2).0.into_iter().map(|l| Line::styled(l, app.theme.muted())).collect();
    lines.push(Line::default());
    lines
}

/// The browser's menu path, kept on one line.
const MENU: &str = "Copy > Copy as cURL";

/// A numbered step (`2. Open …`) wrapped to `w` columns: its wrapped lines go on under its
/// text, not under its number, and the menu path is never broken.
fn step_lines(step: &str, w: usize) -> Vec<String> {
    let (number, body) = match step.split_once(". ") {
        Some((n, b)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => (format!("{n}. "), b),
        _ => (String::new(), step),
    };
    let indent = " ".repeat(crate::text::width(&number));
    // The same width, with no space to break at (and back once wrapped).
    let joined = MENU.replace(' ', "_");
    let body = body.replace(MENU, &joined);
    let w = w.saturating_sub(indent.len()).max(8);
    wrap(&body, w, 4)
        .0
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let l = l.replace(&joined, MENU);
            if i == 0 { format!("{number}{l}") } else { format!("{indent}{l}") }
        })
        .collect()
}
