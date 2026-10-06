//! A question over the dimmed screen, two answers below it; the safe one has the focus.
//!
//! ```text
//! ╭ Quit slakio? ─────────────────────────────╮
//! │ A message you wrote has not been sent.     │
//! │                                            │
//! │            [ Stay ]   [ Quit ]             │
//! ╰────────────────────── Enter Stay · y Quit ─╯
//! ```

use super::modal;
use crate::app::App;
use crate::app::dialog::Question;
use crate::text::wrap;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};

/// The glyphs the icons question shows (the view switcher's and the list's).
const PREVIEW: &str = "\u{F02DC}  \u{F0361}  \u{F009A}  \u{F0219}  \u{F00C0}  \u{F033E}  \u{F0339}";

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let Some(d) = app.dialog else { return };
    let (title, text, no, yes) = match d.question {
        Question::Quit => (Label::DialogQuitTitle, Label::DialogQuitText, Label::DialogQuitNo, Label::DialogQuitYes),
        Question::QuitConfirm => {
            (Label::DialogQuitTitle, Label::DialogQuitConfirmText, Label::DialogQuitNo, Label::DialogQuitYes)
        }
        Question::Icons => {
            (Label::DialogIconsTitle, Label::DialogIconsText, Label::DialogIconsNo, Label::DialogIconsYes)
        }
    };
    let (no, yes) = (app.i18n.label(no).to_string(), app.i18n.label(yes).to_string());
    let w = area.width.saturating_sub(4).min(60);
    let inner_w = usize::from(w.saturating_sub(4)).max(8);
    let mut lines: Vec<Line> =
        wrap(&app.i18n.label(text), inner_w, 6).0.into_iter().map(|l| Line::styled(l, t.text())).collect();
    if d.question == Question::Icons {
        lines.push(Line::default());
        lines.push(Line::styled(PREVIEW, t.bold()).alignment(Alignment::Center));
        lines.push(Line::default());
        let later = app.i18n.label(Label::DialogIconsLater);
        lines.extend(wrap(&later, inner_w, 3).0.into_iter().map(|l| Line::styled(l, t.muted())));
    }
    lines.push(Line::default());
    let button = |label: &str, focused: bool| {
        let style = if focused { t.mode(t.accent).add_modifier(Modifier::BOLD) } else { t.muted() };
        Span::styled(format!("[ {label} ]"), style)
    };
    lines.push(
        Line::from(vec![button(&no, !d.yes), Span::raw("   "), button(&yes, d.yes)]).alignment(Alignment::Center),
    );
    let h = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + area.height.saturating_sub(h) / 2, w, h);
    let footer = app.i18n.msg(&Msg::DialogKeys { no: no.clone(), yes: yes.clone() }).to_string();
    let inner = modal(f, app, rect, &app.i18n.label(title), &footer);
    let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    f.render_widget(Paragraph::new(lines), inner);
}
