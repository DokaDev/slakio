//! The sign-in screen: how to copy a request from the browser, what happens to it, and where
//! signing in stands (checking, or why the last paste did not sign in). Then the keys to quit
//! and to open the help.

use super::empty::Content;
use crate::app::App;
use crate::app::signin::{Picker, SignIn};
use crate::text::wrap;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use slakio_core::i18n::Label;

/// The widest the text runs, for lines that read well.
const WIDTH: u16 = 76;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect, state: SignIn) {
    match state {
        SignIn::Desktop(p) => draw_picker(f, app, area, &p, false),
        SignIn::DesktopChecking(p) => draw_picker(f, app, area, &p, true),
        other => draw_paste(f, app, area, other),
    }
}

/// The desktop-workspace picker: the workspaces the Slack app is signed in to, one per line, with
/// the cursor, how to choose, and the keychain-prompt note.
fn draw_picker(f: &mut Frame, app: &App, area: Rect, p: &Picker, checking: bool) {
    let t = &app.theme;
    let w = area.width.saturating_sub(4).min(WIDTH);
    let text = |l: Label| app.i18n.label(l).to_string();
    let mut lines: Vec<Line<'static>> =
        vec![Line::styled(text(Label::SigninDesktopTitle), t.current()), Line::default()];
    lines.extend(
        wrap(&text(Label::SigninDesktopFound), usize::from(w), 2).0.into_iter().map(|l| Line::styled(l, t.text())),
    );
    lines.push(Line::default());
    for (i, name) in p.names.iter().enumerate() {
        let picked = i == p.selected;
        let marker = if picked { "> " } else { "  " };
        let style = if picked { t.accent() } else { t.text() };
        lines.push(Line::styled(format!("{marker}{name}"), style));
    }
    lines.push(Line::default());
    let (said, style) = if checking {
        (Label::SigninChecking, t.accent())
    } else if let Some(note) = p.note {
        (note, t.warning())
    } else {
        (Label::SigninDesktopKeys, t.faint())
    };
    lines.extend(wrap(&text(said), usize::from(w), 3).0.into_iter().map(|l| Line::styled(l, style)));
    lines.push(Line::default());
    lines.extend(
        wrap(&text(Label::SigninDesktopNote), usize::from(w), 4).0.into_iter().map(|l| Line::styled(l, t.muted())),
    );
    render_centered(f, app, area, w, lines);
}

/// Center `lines` in `area` with the quit/help keys below, the common frame of the sign-in
/// screens.
fn render_centered(f: &mut Frame, app: &App, area: Rect, w: u16, lines: Vec<Line<'static>>) {
    use crate::action::{Action, AppAction, HelpAction};
    use crate::keymap::Ctx;
    let keys = Content::keys(
        app,
        Ctx::Root,
        &[(Action::App(AppAction::Quit), Label::EmptyQuit), (Action::Help(HelpAction::Open), Label::EmptyHelp)],
    );
    let keys = Content { headline: None, sentence: None, keys, tip: None };
    let h = lines.len() as u16;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h + 4) / 2;
    let at = Rect::new(x, y, w, h.min(area.height)).intersection(area);
    f.render_widget(Paragraph::new(lines), at);
    let below = Rect::new(x, at.bottom().saturating_add(1), w, 3).intersection(area);
    keys.draw_top(f, app, Rect { x: below.x.saturating_sub(1), width: below.width + 2, ..below });
}

fn draw_paste(f: &mut Frame, app: &App, area: Rect, state: SignIn) {
    let t = &app.theme;
    let w = area.width.saturating_sub(4).min(WIDTH);
    let text = |l: Label| app.i18n.label(l).to_string();
    let mut lines: Vec<Line<'static>> = vec![Line::styled(text(Label::SigninTitle), t.current()), Line::default()];
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
