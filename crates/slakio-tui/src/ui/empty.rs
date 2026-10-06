//! Empty states that teach: a bold headline, one muted sentence, then the keys that get the user
//! somewhere, each looked up in the key map (so a rebound key shows as bound), and a muted tip.
//!
//! ```text
//! No conversation open
//! Pick one in the list and press Enter.
//!
//! Enter    Open
//! Space h  Home
//! ?        Keyboard help
//! ```

use crate::action::Action;
use crate::app::App;
use crate::app::shell::View;
use crate::keymap::Ctx;
use crate::text::{clip, width, wrap};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};

/// The keys that run `action` from `ctx`, as shown ([`crate::keymap::hints::key_label`]).
/// `None` when nothing runs it.
pub(super) fn key_of(app: &App, action: Action, ctx: Ctx) -> Option<String> {
    crate::keymap::hints::key_label(&app.keymap, action, ctx)
}

/// What an empty state says.
pub(super) struct Content {
    pub headline: Option<(String, Style)>,
    pub sentence: Option<String>,
    /// (keys, label) rows; an action nothing is bound to is left out by the caller.
    pub keys: Vec<(String, String)>,
    pub tip: Option<String>,
}

impl Content {
    /// The key rows of `actions` (run from `ctx`), each labelled.
    pub fn keys(app: &App, ctx: Ctx, actions: &[(Action, Label)]) -> Vec<(String, String)> {
        actions.iter().filter_map(|(a, l)| key_of(app, *a, ctx).map(|k| (k, app.i18n.label(*l).to_string()))).collect()
    }

    fn lines(&self, app: &App, w: usize) -> Vec<Line<'static>> {
        let t = &app.theme;
        let mut out = Vec::new();
        // Words wrap; a narrow list panel still shows the whole sentence.
        if let Some((h, style)) = &self.headline {
            out.extend(wrap(h, w, 3).0.into_iter().map(|l| Line::styled(l, *style)));
        }
        if let Some(s) = &self.sentence {
            out.extend(wrap(s, w, 3).0.into_iter().map(|l| Line::styled(l, t.muted())));
        }
        if !self.keys.is_empty() {
            if !out.is_empty() {
                out.push(Line::default());
            }
            let key_w = self.keys.iter().map(|(k, _)| width(k)).max().unwrap_or(0).max(4) + 2;
            for (k, l) in &self.keys {
                let pad = " ".repeat(key_w.saturating_sub(width(k)));
                let label = clip(l, w.saturating_sub(key_w));
                out.push(Line::from(vec![
                    Span::styled(k.clone(), t.key_warm()),
                    Span::raw(pad),
                    Span::styled(label, t.text()),
                ]));
            }
        }
        if let Some(tip) = &self.tip {
            out.push(Line::default());
            out.extend(wrap(tip, w, 3).0.into_iter().map(|l| Line::styled(l, t.muted())));
        }
        out
    }

    /// Draw from the top left of `area`, inside one cell of padding.
    pub fn draw_top(&self, f: &mut Frame, app: &App, area: Rect) {
        let area = Rect { x: area.x + 1, width: area.width.saturating_sub(2), ..area };
        let lines = self.lines(app, usize::from(area.width));
        f.render_widget(Paragraph::new(lines), area);
    }

    /// Draw as a block in the middle of `area` (its lines left aligned).
    pub fn draw_centered(&self, f: &mut Frame, app: &App, area: Rect) {
        let lines = self.lines(app, usize::from(area.width.saturating_sub(2)));
        let w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
        let h = lines.len() as u16;
        let x = area.x + area.width.saturating_sub(w) / 2;
        let y = area.y + area.height.saturating_sub(h) / 2;
        let at = Rect::new(x, y, w.min(area.width), h.min(area.height)).intersection(area);
        f.render_widget(Paragraph::new(lines), at);
    }
}

/// The list panel with nothing to list: a view of a later version, or a view with no rows.
pub(super) fn list(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let view = app.shell.view;
    let headline = if view.is_placeholder() {
        let name = app.i18n.label(super::view_label(view)).to_string();
        app.i18n.msg(&Msg::ListLaterVersion { view: name }).to_string()
    } else {
        app.i18n.label(Label::ListEmpty).to_string()
    };
    let others: Vec<(Action, Label)> = [(View::Home, Label::NavHome), (View::Dms, Label::NavDms)]
        .into_iter()
        .filter(|(v, _)| *v != view)
        .map(|(v, l)| (Action::Shell(crate::action::ShellAction::Show(v)), l))
        .collect();
    let content = Content {
        headline: Some((headline, t.bold())),
        sentence: None,
        keys: Content::keys(app, Ctx::List, &others),
        tip: None,
    };
    content.draw_top(f, app, area);
}

/// The work area with no conversation open.
pub(super) fn work(f: &mut Frame, app: &App, area: Rect) {
    use crate::action::{AppAction, HelpAction, PaneAction, ShellAction};
    let t = &app.theme;
    let mut keys = Content::keys(app, Ctx::List, &[(Action::Shell(ShellAction::ListOpen), Label::EmptyOpen)]);
    keys.extend(Content::keys(
        app,
        Ctx::PaneNormal,
        &[
            (Action::Shell(ShellAction::Show(View::Home)), Label::NavHome),
            (Action::Shell(ShellAction::Show(View::Dms)), Label::NavDms),
            (Action::Pane(PaneAction::Back), Label::EmptyBack),
            (Action::Help(HelpAction::Open), Label::EmptyHelp),
            (Action::App(AppAction::Quit), Label::EmptyQuit),
        ],
    ));
    let content = Content {
        headline: Some((app.i18n.label(Label::EmptyNoConversation).to_string(), t.bold())),
        sentence: Some(app.i18n.label(Label::EmptyPick).to_string()),
        keys,
        tip: None,
    };
    content.draw_centered(f, app, area);
}
