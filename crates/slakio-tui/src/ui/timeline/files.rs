//! The files a message shares, as cards under its text: one row each, an icon (a downward arrow
//! without Nerd Font icons: the UI draws no emoji), the name, and its kind and size in muted
//! text; a file that cannot be downloaded here (hosted elsewhere, deleted) has an arrow out
//! instead and says so instead of its size. Short of room ([`fit`]), the name keeps half of it,
//! the kind goes before the size, and the size before the name; a name cut keeps its extension.
//! `f` downloads one ([`crate::app::files`]).
//!
//! ```text
//!       󰈙 Q3-report.pdf  PDF · 1.2 MB
//!       ⇗ Shared plan  Google Docs · not on Slack
//! ```

use super::message::{Kind, Laid};
use crate::app::App;
use crate::app::files::size_words;
use crate::app::pane::Shown;
use crate::text::{clip_name, width};
use ratatui::text::{Line, Span};
use slakio_core::i18n::Label;

/// The icon before a file's name: a Nerd Font file, else a downward arrow; for one that is not
/// downloaded here, an arrow out (one cell wide).
fn icon(app: &App, here: bool) -> &'static str {
    match (app.settings.icons, here) {
        (true, true) => "\u{F0214} ",
        (true, false) => "\u{F03CC} ",
        (false, true) => "\u{21E9} ",
        (false, false) => "\u{21D7} ",
    }
}

/// The least of a file's name kept before its kind and size, when it is that long.
const NAME_KEPT: usize = 12;

/// A file's `name` and its kind and size (`meta`, the kind first) in `inside` cells, two apart:
/// the name keeps half of them (at least [`NAME_KEPT`]; all it needs when shorter), the kind goes
/// before the size, and a size that does not fit whole goes too: it is never cut. The name is cut
/// in its middle, its extension kept.
pub fn fit(name: &str, mut meta: Vec<String>, inside: usize) -> (String, String) {
    let keep = width(name).min((inside / 2).max(NAME_KEPT));
    let room = inside.saturating_sub(keep + 2);
    while meta.len() > 1 && width(&meta.join(" · ")) > room {
        meta.remove(0);
    }
    let meta = meta.join(" · ");
    if meta.is_empty() || width(&meta) > room {
        return (clip_name(name, inside), String::new());
    }
    (clip_name(name, inside - 2 - width(&meta)), meta)
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
            let icon = icon(app, f.download.is_some());
            let (name, meta) = fit(f.name.line().as_str(), meta, room.saturating_sub(width(icon)));
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
