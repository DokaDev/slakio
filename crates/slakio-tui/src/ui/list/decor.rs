//! What follows a DM's name in the list: on the user's own DM, `you` (muted, as Slack writes
//! it), then (after an app's or another organization's tag) the peer's status emoji. A standard emoji is its glyph; one of the workspace's own
//! is its picture's placeholder cells once the terminal holds it (their colour says which
//! picture, so nothing restyles them), else `:name:`. A status past its end is not shown. Short
//! of room, the status goes first, then `you`, then the name is cut with `…`.
//!
//! ```text
//! │   ME● Me you ee             │   ee = the status emoji's glyph (two cells)
//! │   MK● Minsu Kim ee      3   │
//! │   JP● Jiho Park ▒▒          │   ▒▒ = a custom emoji's picture
//! ```

use crate::app::App;
use crate::text::{clip, width};
use ratatui::style::Style;
use ratatui::text::Span;
use slakio_core::i18n::Label;
use slakio_core::model::{Conversation, ConversationKind};

/// What a DM row shows after its name: `you` on the user's own DM, and the peer's status as
/// runs (each with its style).
#[derive(Default)]
pub(super) struct Decor {
    you: Option<String>,
    status: Vec<(String, Style)>,
}

/// The decor of conversation `c`'s row, its words in `style`: none but for a DM. `you` only
/// when the user's own account in the workspace is known and is the peer.
pub(super) fn of(app: &App, c: &Conversation, style: Style) -> Decor {
    let ConversationKind::Dm { user } = &c.kind else { return Decor::default() };
    let you = (app.model.me(&c.workspace) == Some(user)).then(|| app.i18n.label(Label::ListYou).to_string());
    let now = app.now_ts().0 / 1_000_000;
    let status = app.model.user(&c.workspace, user).and_then(|u| u.status.as_ref()).filter(|s| s.holds(now));
    let status = status.map_or_else(Vec::new, |s| {
        let name = s.emoji.line().into_string();
        match crate::emoji::get(&name) {
            Some(glyph) => vec![(glyph.to_string(), style)],
            None => crate::ui::timeline::custom::runs(app, &c.workspace, &format!(":{name}:"), style, &[]),
        }
    });
    Decor { you, status }
}

/// `name` (in `style`) and its decor in at most `room` cells: all of it, else without the status,
/// else without `you` either, the name cut with `…`. The name with `you` and its width, then the
/// status (after the row's tags, whose room is kept apart) and its width.
pub(super) fn fit(name: &str, room: usize, d: Decor, style: Style, muted: Style) -> Fitted {
    let you = d.you.map(|y| format!(" {y}"));
    let you_w = you.as_deref().map_or(0, width);
    let status_w = d.status.iter().map(|(s, _)| width(s)).sum::<usize>();
    let status_w = if status_w > 0 { status_w + 1 } else { 0 };
    let name_w = width(name);
    let (keep_you, keep_status) = match () {
        () if name_w + you_w + status_w <= room => (true, true),
        () if name_w + you_w <= room => (true, false),
        () => (false, false),
    };
    let name = if keep_you { name.to_string() } else { clip(name, room) };
    let mut head_w = width(&name);
    let mut head = vec![Span::styled(name, style)];
    if let Some(you) = you.filter(|_| keep_you) {
        head.push(Span::styled(you, muted));
        head_w += you_w;
    }
    let mut tail = Vec::new();
    if keep_status && status_w > 0 {
        tail.push(Span::styled(" ", muted));
        tail.extend(d.status.into_iter().map(|(s, st)| Span::styled(s, st)));
    }
    let tail_w = if tail.is_empty() { 0 } else { status_w };
    ((head, head_w), (tail, tail_w))
}

/// What [`fit`] gives: the name with `you`, and the status, each with its width.
pub(super) type Fitted = ((Vec<Span<'static>>, usize), (Vec<Span<'static>>, usize));
