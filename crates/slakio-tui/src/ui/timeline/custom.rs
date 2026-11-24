//! A workspace's own emoji in a message's text and its reaction chips ([`crate::images::custom`]):
//! a `:name:` the standard emoji do not know is drawn as the workspace's picture for it, two
//! cells of placeholders ([`graphics::row`], in the picture's id colour) once the terminal holds
//! it; an alias of a standard emoji is drawn as that emoji. Everywhere else — pictures off (a
//! terminal that draws none, one inside a multiplexer), the list or the picture not there yet,
//! a picture that failed or is on another host, past the frame's emoji pictures
//! ([`crate::images::EMOJI_PER_FRAME`]) — the `:name:` stays text, as before. Only what a
//! frame draws asks for a picture ([`crate::images::Images::look_as`]): a message scrolled away,
//! a pane closed, `:images off`, and its cells are gone with it.

use crate::app::App;
use crate::emoji;
use crate::images::{Look, graphics};
use ratatui::style::Style;
use slakio_core::images::emoji::{Custom, shortcode};
use slakio_core::model::WorkspaceId;

/// The runs `text` (in `style`) is drawn as in workspace `ws`: the text as it is, but for each
/// `:name:` of the workspace's own emoji that can be drawn (as its picture, or the standard
/// emoji it is an alias of).
pub fn runs(app: &App, ws: &WorkspaceId, text: &str, style: Style) -> Vec<(String, Style)> {
    let mut out: Vec<(String, Style)> = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        plain.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let name = after.find(':').map(|e| &after[..e]).filter(|n| shortcode(n));
        match name.and_then(|n| drawn(app, ws, n, style)) {
            Some((mut run, picture)) => {
                if !plain.is_empty() {
                    out.push((std::mem::take(&mut plain), style));
                }
                rest = &after[name.unwrap_or_default().len() + 1..];
                if picture {
                    rest = after_picture(rest);
                } else if let Some((toned, past)) = toned(app, ws, name.unwrap_or_default(), rest) {
                    (run.0, rest) = (toned.to_string(), past);
                }
                out.push(run);
            }
            None => {
                plain.push(':');
                rest = after;
            }
        }
    }
    plain.push_str(rest);
    if !plain.is_empty() || out.is_empty() {
        out.push((plain, style));
    }
    out
}

/// `rest`, the text right after a picture, without what would only spoil it: a skin tone
/// (`:skin-tone-2:` to `-6:`, which a picture has no way to take) and the marks that join the
/// character before them (a combining accent, a variation selector), which would join the
/// picture's last cell and blank it.
fn after_picture(rest: &str) -> &str {
    let rest = (rest.strip_prefix(":skin-tone-"))
        .and_then(|r| r.strip_prefix(['2', '3', '4', '5', '6']))
        .and_then(|r| r.strip_prefix(':'))
        .unwrap_or(rest);
    rest.trim_start_matches(|c: char| c != '\n' && unicode_width::UnicodeWidthChar::width(c) == Some(0))
}

/// The standard emoji the alias `name` of workspace `ws` stands for, in the skin tone `rest` (the
/// text right after it) starts with (`:skin-tone-2:` to `-6:`), and the text past the tone.
fn toned<'a>(app: &App, ws: &WorkspaceId, name: &str, rest: &'a str) -> Option<(&'static str, &'a str)> {
    let tone = rest.strip_prefix(":skin-tone-")?;
    let digit = tone.chars().next().filter(|d| ('2'..='6').contains(d))?;
    let past = tone[1..].strip_prefix(':')?;
    let Some(Custom::Standard(standard)) = app.images.customs.get(ws, name) else { return None };
    emoji::get(&format!("{standard}::skin-tone-{digit}")).map(|e| (e, past))
}

/// What `:name:` is drawn as in workspace `ws`, when it is one of the workspace's own emoji that
/// can be drawn now (`true`: as its picture).
fn drawn(app: &App, ws: &WorkspaceId, name: &str, style: Style) -> Option<((String, Style), bool)> {
    if emoji::get(name).is_some() {
        return None;
    }
    match app.images.customs.get(ws, name)? {
        Custom::Standard(standard) => emoji::get(standard).map(|e| ((e.to_string(), style), false)),
        Custom::Picture(address) => match app.images.look_emoji(ws, address) {
            Look::Ready { id, cols: 2, rows: 1 } => {
                let mut cells = Style::default().fg(graphics::color(id));
                cells.bg = style.bg;
                Some(((graphics::row(0, 2), cells), true))
            }
            _ => None,
        },
    }
}
