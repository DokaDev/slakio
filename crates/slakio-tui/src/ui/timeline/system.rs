//! Slack's notices about a conversation (someone joined or left, the topic was set, the channel
//! was renamed or archived, a message pinned, an app added) as one quiet line, as GUI Slack
//! draws them: a small icon in the gap before the text column, the words in muted text, then
//! the time. No picture, no name header, never grouped with the messages around it; what is
//! under it (files, the "N replies" link, the reactions) stays.
//!
//! ```text
//!     - Minsu Kim left #backend.  10:02            (icons off)
//!     󰀔 Minsu Kim joined #backend.  10:03           (icons on)
//! ```

use super::message::{Kind, Laid};
use crate::app::App;
use crate::app::pane::{Pane, Shown};
use crate::app::timelines::Timeline;
use crate::mrkdwn::{Piece, first_line, parse};
use crate::text::{clip, width};
use crate::time;
use ratatui::text::{Line, Span};
use slakio_core::i18n::Msg;
use slakio_core::model::System;
use slakio_core::sanitize::Safe;

/// The icon of a notice: a Nerd Font glyph with icons on, else an ASCII mark (one cell wide).
fn icon(app: &App, s: &System<Safe>) -> &'static str {
    let (glyph, mark) = match s {
        System::Joined => ("\u{F0014}", "+"),
        System::Left => ("\u{F0015}", "-"),
        System::Topic(_) | System::Purpose(_) | System::Renamed { .. } => ("\u{F03EB}", "*"),
        System::Archived | System::Unarchived => ("\u{F003C}", "#"),
        System::Pinned => ("\u{F0403}", "^"),
        System::Unpinned => ("\u{F0404}", "^"),
        System::AppAdded | System::AppRemoved => ("\u{F06A9}", "&"),
        System::MadePrivate | System::MadePublic => ("\u{F033E}", "*"),
    };
    if app.settings.icons { glyph } else { mark }
}

/// What notice `s` of message `m` says: by its author, about the pane's conversation (a DM's or
/// a group DM's in its own words). A topic or a description is Slack markup, read as plain text
/// on one line with its people and channels named, as the timeline names them.
fn words(app: &App, pane: &Pane, m: &Shown, s: &System<Safe>) -> String {
    let name = m.author.as_str().to_string();
    let conversation = app.model.target(&pane.target);
    let channel = || conversation.map(crate::app::model::breadcrumb).unwrap_or_default();
    let dm = conversation.is_some_and(|c| c.is_dm());
    let text = |t: &Safe| t.as_str().to_string();
    let markup = |t: &Safe| {
        let named = |p: &Piece| app.model.named(pane.target.workspace(), p);
        let blocks = parse(t.as_str());
        let lines = blocks.iter().filter_map(|b| first_line(std::slice::from_ref(b), &named));
        lines.map(|l| l.trim().to_string()).collect::<Vec<_>>().join(" ")
    };
    let msg = match s {
        System::Joined => Msg::SystemJoined { name, channel: channel() },
        System::Left => Msg::SystemLeft { name, channel: channel() },
        System::Topic(t) if dm => Msg::SystemTopicDm { name, text: markup(t) },
        System::Topic(t) => Msg::SystemTopic { name, text: markup(t) },
        System::Purpose(t) if dm => Msg::SystemPurposeDm { name, text: markup(t) },
        System::Purpose(t) => Msg::SystemPurpose { name, text: markup(t) },
        System::Renamed { old, new } => Msg::SystemRenamed { name, old: text(old), new: text(new) },
        System::Archived => Msg::SystemArchived { name, channel: channel() },
        System::Unarchived => Msg::SystemUnarchived { name, channel: channel() },
        System::Pinned if dm => Msg::SystemPinnedDm { name },
        System::Pinned => Msg::SystemPinned { name },
        System::Unpinned if dm => Msg::SystemUnpinnedDm { name },
        System::Unpinned => Msg::SystemUnpinned { name },
        System::AppAdded if dm => Msg::SystemAppAddedDm { name },
        System::AppAdded => Msg::SystemAppAdded { name },
        System::AppRemoved if dm => Msg::SystemAppRemovedDm { name },
        System::AppRemoved => Msg::SystemAppRemoved { name },
        System::MadePrivate => Msg::SystemPrivate { name, channel: channel() },
        System::MadePublic => Msg::SystemPublic { name, channel: channel() },
    };
    app.i18n.msg(&msg).to_string()
}

/// The row of notice `i` of `pane` for a width of `w` cells: the icon two cells before the text
/// column (at least two cells in), the words cut with `…` so the time always fits after them.
pub fn rows(app: &App, pane: &Pane, tl: &Timeline, i: usize, w: usize) -> Vec<Laid> {
    let t = &app.theme;
    let m = &tl.items[i];
    let Some(s) = &m.system else { return Vec::new() };
    let lead = super::message::indent(app, w).max(2);
    let time = time::hm(m.ts, &app.zone);
    let room = w.saturating_sub(lead + 2 + width(&time)).max(1);
    let text = clip(&words(app, pane, m, s), room);
    let line = Line::from(vec![
        Span::raw(" ".repeat(lead - 2)),
        Span::styled(icon(app, s), t.muted()),
        Span::raw(" "),
        Span::styled(text, t.muted()),
        Span::raw("  "),
        Span::styled(time, t.faint()),
    ]);
    vec![(line, Kind::Content, Vec::new())]
}
