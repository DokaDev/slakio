//! The list of a message's files to download (`f`, when it shares several): each row its
//! letters (faint once the letters typed do not start them), the file's name and its kind and
//! size after it; the cursor row drawn as the selection. It scrolls with its cursor; when it has
//! more rows than it shows, its title says where the cursor is (`3/30`).
//!
//! ```text
//! ╭ Download which file? ────────────────────────╮
//! │ a  Q3-report.pdf      PDF · 1.2 MB            │
//! │ s  login-screen.png   PNG · 346 KB            │
//! ╰─────────── j/k move · Enter download · Esc cancel ─╯
//! ```

use super::modal;
use crate::app::App;
use crate::app::files::size_words;
use crate::text::{clip, width};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::Label;

pub(super) fn draw(f: &mut Frame, app: &App) {
    let t = &app.theme;
    let (Some(p), Some((rect, _))) = (app.file_picker(), app.file_rects()) else { return };
    let label = |l: Label| app.i18n.label(l).to_string();
    let footer = format!(
        "j/k {} · Enter {} · Esc {}",
        label(Label::HintMove),
        label(Label::HintDownload),
        label(Label::HintCancel)
    );
    let rows = usize::from(crate::screen::inner(rect).height);
    let mut title = label(Label::FileListTitle);
    if p.files.len() > rows {
        title = format!("{title} {}/{}", p.cursor + 1, p.files.len());
    }
    let inner = modal(f, app, rect, &title, &footer);
    let w = usize::from(inner.width);
    let top = app.file_top(rows);
    let lead_w = p.hints.iter().map(|h| width(h)).max().unwrap_or(0) + 2;
    for (r, (i, file)) in p.files.iter().enumerate().skip(top).take(rows).enumerate() {
        let row = Rect { y: inner.y + r as u16, height: 1, ..inner };
        let hint = p.hints.get(i).cloned().unwrap_or_default();
        let on = hint.starts_with(&p.typed);
        let lead = format!(" {hint:<pad$} ", pad = lead_w - 2);
        let kind = file.kind.line();
        let meta = match (kind.is_empty(), file.size) {
            (false, Some(s)) => format!("{} · {}", kind.as_str(), size_words(s)),
            (false, None) => kind.into_string(),
            (true, Some(s)) => size_words(s),
            (true, None) => String::new(),
        };
        let meta = clip(&meta, w.saturating_sub(lead_w + 12) / 2);
        let name = clip(file.name.line().as_str(), w.saturating_sub(lead_w + 4 + width(&meta)));
        let gap = w.saturating_sub(lead_w + 2 + width(&name) + width(&meta)).max(2);
        let line = Line::from(vec![
            Span::styled(lead, if on { t.hint() } else { t.faint() }),
            Span::raw(" "),
            Span::styled(name, t.bold()),
            Span::raw(" ".repeat(gap)),
            Span::styled(meta, t.muted()),
        ]);
        f.render_widget(Paragraph::new(line), row);
        if i == p.cursor {
            let x = lead_w as u16;
            let rest = Rect { x: row.x + x, width: row.width.saturating_sub(x), ..row };
            t.paint_selection(f.buffer_mut(), rest, Selection::Focused);
        }
    }
}
