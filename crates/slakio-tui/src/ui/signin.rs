//! The sign-in screen: how to copy a request from the browser, what happens to it, and where
//! signing in stands (checking, or why the last paste did not sign in). Then the keys to quit
//! and to open the help.

use super::empty::Content;
use crate::app::App;
use crate::app::signin::SignIn;
use crate::text::wrap;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use slakio_core::i18n::Label;

/// The widest the text runs, for lines that read well.
const WIDTH: u16 = 76;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect, state: SignIn) {
    use crate::action::{Action, AppAction, HelpAction};
    use crate::keymap::Ctx;
    let t = &app.theme;
    let w = area.width.saturating_sub(4).min(WIDTH);
    let text = |l: Label| app.i18n.label(l).to_string();
    let mut lines: Vec<Line<'static>> = vec![Line::styled(text(Label::SigninTitle), t.current()), Line::default()];
    for step in [Label::SigninStep1, Label::SigninStep2, Label::SigninStep3, Label::SigninStep4] {
        lines.extend(wrap(&text(step), usize::from(w), 3).0.into_iter().map(|l| Line::styled(l, t.text())));
    }
    lines.push(Line::default());
    lines.extend(wrap(&text(Label::SigninNote), usize::from(w), 4).0.into_iter().map(|l| Line::styled(l, t.muted())));
    let (said, style) = match state {
        SignIn::Waiting => (None, t.text()),
        SignIn::Checking => (Some(Label::SigninChecking), t.accent()),
        SignIn::Failed(why) => (Some(why), t.warning()),
    };
    if let Some(l) = said {
        lines.push(Line::default());
        lines.extend(wrap(&text(l), usize::from(w), 3).0.into_iter().map(|line| Line::styled(line, style)));
    }
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
