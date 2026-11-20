//! The files a message shares, as cards under its text: one row each, an icon (a downward arrow
//! without Nerd Font icons: the UI draws no emoji), the name, and its kind and size in muted
//! text; a file that cannot be downloaded here (hosted elsewhere, deleted) has an arrow out
//! instead and says so instead of its size. Short of room ([`fit`]), the name keeps half of it,
//! the kind goes before the size, and the size before the name; a name cut keeps its extension.
//! `f` downloads one ([`crate::app::files`]).
//!
//! Where pictures are drawn ([`crate::images`]), a picture's card has its preview under it once
//! it came (when the pane is wide enough for it), or says on the card why there is none.
//!
//! ```text
//!       󰈙 Q3-report.pdf  PDF · 1.2 MB
//!       ⇗ Shared plan  Google Docs · not on Slack
//! ```

use super::message::{Kind, Laid};
use crate::app::App;
use crate::app::files::size_words;
use crate::app::pane::Shown;
use crate::images::{Look, Why, graphics};
use crate::text::{clip_name, width};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use slakio_core::i18n::Label;
use slakio_core::images::Source;
use slakio_core::model::WorkspaceId;

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

/// The rows of picture `id`, `cols` × `rows` cells, `indent` cells in.
pub fn picture_rows(id: u32, cols: u16, rows: u16, indent: usize) -> Vec<Laid> {
    let style = Style::default().fg(graphics::color(id));
    (0..rows)
        .map(|r| {
            let spans = vec![Span::raw(" ".repeat(indent)), Span::styled(graphics::row(r, cols), style)];
            (Line::from(spans), Kind::Content, Vec::new())
        })
        .collect()
}

/// A picture ready is no wider than `room` cells.
fn fits(look: &Look, room: usize) -> bool {
    matches!(look, Look::Ready { cols, .. } if usize::from(*cols) <= room)
}

/// What a picture's card says when it has no preview.
fn why_words(app: &App, why: Why) -> String {
    let label = match why {
        Why::TooLarge => Label::FilePreviewTooLarge,
        Why::Unreadable => Label::FilePreviewUnreadable,
        Why::NotLoaded => Label::FilePreviewNotLoaded,
    };
    app.i18n.label(label).to_string()
}

/// The cards of `m`'s files (of workspace `ws`), `indent` cells in, `w` cells wide in all; a
/// picture's preview under its card.
pub fn cards(app: &App, ws: &WorkspaceId, m: &Shown, indent: usize, w: usize) -> Vec<Laid> {
    let t = &app.theme;
    let room = w.saturating_sub(indent);
    m.files
        .iter()
        .flat_map(|f| {
            let look = f.thumb.as_ref().map(|a| app.images.look(ws, &Source::Thumb(a.clone())));
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
            if let Some(Look::Failed(why)) = look {
                meta.push(why_words(app, why));
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
            let mut rows = vec![(Line::from(spans), Kind::Content, Vec::new())];
            if let Some(Look::Ready { id, cols, rows: down }) = look.filter(|l| fits(l, room)) {
                rows.extend(picture_rows(id, cols, down, indent));
            }
            rows
        })
        .collect()
}
