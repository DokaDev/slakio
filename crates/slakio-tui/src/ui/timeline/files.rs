//! The files a message shares, as cards under its text: one row each, an icon (a downward arrow
//! without Nerd Font icons: the UI draws no emoji), the name, and its kind and size in muted text; a file that cannot
//! be downloaded here (hosted elsewhere, deleted) says so instead of its size. Short of room,
//! the name keeps half of it and the kind goes before the size. `f` downloads one
//! ([`crate::app::files`]).
//!
//! ```text
//!       󰈙 Q3-report.pdf  PDF · 1.2 MB
//!       󰈙 Shared plan  Google Docs · not on Slack
//! ```

use super::message::{Kind, Laid};
use crate::app::App;
use crate::app::files::size_words;
use crate::app::pane::Shown;
use crate::text::{clip, width};
use ratatui::text::{Line, Span};
use slakio_core::i18n::Label;

/// The icon before a file's name: a Nerd Font file, else a downward arrow (one cell wide).
fn icon(app: &App) -> &'static str {
    if app.settings.icons { "\u{F0214} " } else { "\u{21E9} " }
}

/// The cards of `m`'s files, `indent` cells in, `w` cells wide in all.
pub fn cards(app: &App, m: &Shown, indent: usize, w: usize) -> Vec<Laid> {
    let t = &app.theme;
    let room = w.saturating_sub(indent);
    m.files
        .iter()
        .map(|f| {
            let kind = f.kind.line();
            let mut meta: Vec<String> = Vec::new();
            if !kind.is_empty() {
                meta.push(kind.into_string());
            }
            match (f.download.is_some(), f.size) {
                (false, _) => meta.push(app.i18n.label(Label::FileElsewhere).to_string()),
                (true, Some(s)) => meta.push(size_words(s)),
                (true, None) => {}
            }
            let icon = icon(app);
            // The name keeps at least half the room (all it needs when it is short); the kind and
            // size get what is left.
            let name = f.name.line();
            let inside = room.saturating_sub(width(icon) + 2);
            let meta_room = inside - width(name.as_str()).min(inside / 2);
            // Short of room, the kind goes first: the size (or that it is elsewhere) stays.
            while meta.len() > 1 && width(&meta.join(" · ")) > meta_room {
                meta.remove(0);
            }
            let meta = clip(&meta.join(" · "), meta_room);
            let name = clip(name.as_str(), inside.saturating_sub(width(&meta)).max(1));
            let spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(icon, t.muted()),
                Span::styled(name, t.text()),
                Span::raw("  "),
                Span::styled(meta, t.muted()),
            ];
            (Line::from(spans), Kind::Content, Vec::new())
        })
        .collect()
}
