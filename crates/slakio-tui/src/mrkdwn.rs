//! Slack's message markup (mrkdwn) read into lines of styled pieces, so a message is drawn as
//! GUI Slack draws it: `*bold*`, `_italic_`, `~strike~`, `` `code` ``, fenced code blocks,
//! `>` quotes, people (`<@U…>`), channels (`<#C…|name>`), `@here` and the like, groups, links
//! (`<url|text>`) and emoji shortcodes.
//!
//! The text read is sanitised already ([`slakio_core::sanitize::Remote::block`]): reading it
//! only cuts it into pieces and decodes Slack's three escapes (`&amp;`, `&lt;`, `&gt;`), so no
//! piece holds a character the sanitiser left out. Ids stay ids: the drawing code names people
//! and channels from the read model when it draws (the names may come later than the message).
//!
//! The rules are Slack's: a mark opens after a blank, punctuation or the start of the line and
//! before a non-blank, closes before a blank, punctuation or the end of the line after a
//! non-blank, never spans lines, and does not nest in itself. A mark with no end is text. The
//! work is linear in the text but for the search of a closing mark, which looks at most
//! [`REACH`] bytes ahead: a hostile text cannot make reading it slow.

use crate::emoji;

/// The farthest a closing mark is looked for (bytes): a mark that ends farther is text.
pub const REACH: usize = 1024;

/// The marks on a piece of text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
}

impl Marks {
    /// These marks and the one delimiter `c` stands for; `None` when it is on already (a mark
    /// does not nest in itself).
    fn with(self, c: char) -> Option<Marks> {
        let mut m = self;
        let on = match c {
            '*' => &mut m.bold,
            '_' => &mut m.italic,
            '~' => &mut m.strike,
            _ => return None,
        };
        if *on {
            return None;
        }
        *on = true;
        Some(m)
    }
}

/// A piece of a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// Text, its escapes decoded and its standard emoji drawn.
    Text(String, Marks),
    /// Inline code, as written.
    Code(String),
    /// A person, by id; the label Slack wrote with it, if any (`<@U1|name>`).
    User { id: String, label: Option<String>, marks: Marks },
    /// A channel, by id; the label Slack wrote with it, if any (`<#C1|general>`).
    Channel { id: String, label: Option<String>, marks: Marks },
    /// `@here`, `@channel`, `@everyone`: the word.
    Broadcast(String),
    /// A user group (`<!subteam^S1|@team>`): its label, if any.
    Group(Option<String>),
    /// A link: its address and the text shown for it, if any (`<https://x|text>`).
    Link { url: String, label: Option<String>, marks: Marks },
}

/// A line of a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// A line of text (a line break ends it); `quote`: a quoted line (`> …`).
    Line { quote: bool, pieces: Vec<Piece> },
    /// A line of a code block, as written (drawn whole or cut, never wrapped).
    Code(String),
}

/// Text `text` (sanitised) as the lines of a message.
pub fn parse(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("```") {
        let after = &rest[open + 3..];
        let Some(close) = after.find("```") else { break };
        lines(rest[..open].strip_suffix('\n').unwrap_or(&rest[..open]), &mut out, open == 0);
        let code = &after[..close];
        let code = code.strip_prefix('\n').unwrap_or(code);
        let code = code.strip_suffix('\n').unwrap_or(code);
        out.extend(code.split('\n').map(|l| Block::Code(decode(l))));
        rest = &after[close + 3..];
        rest = rest.strip_prefix('\n').unwrap_or(rest);
        if rest.is_empty() {
            return out;
        }
    }
    lines(rest, &mut out, false);
    out
}

/// The lines of `s` (no code block in it) onto `out`; nothing for an empty `s` when `skip_empty`.
fn lines(s: &str, out: &mut Vec<Block>, skip_empty: bool) {
    if s.is_empty() && skip_empty {
        return;
    }
    for line in s.split('\n') {
        let quoted = line.strip_prefix("&gt;").or_else(|| line.strip_prefix('>')).filter(|b| quotes(b));
        let (quote, body) = match quoted {
            Some(b) => (true, b.strip_prefix(' ').unwrap_or(b)),
            None => (false, line),
        };
        let mut pieces = Vec::new();
        inline(body, Marks::default(), &mut pieces);
        out.push(Block::Line { quote, pieces });
    }
}

/// A line's `>` starts a quote when what follows it, `rest`, is nothing, a blank, a letter, a
/// mark or a `<…>`: not punctuation that makes a face or a comparison of it (`>_<`, `>=5`).
fn quotes(rest: &str) -> bool {
    rest.chars().next().is_none_or(|c| !c.is_ascii_punctuation() || matches!(c, '*' | '~' | '`' | '<'))
}

/// The pieces of `s` (part of one line) under `marks` onto `out`.
fn inline(s: &str, marks: Marks, out: &mut Vec<Piece>) {
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Piece>| {
        if !text.is_empty() {
            let t = unjoined(emoji::replace(&decode(text)), out);
            out.push(Piece::Text(t, marks));
            text.clear();
        }
    };
    let mut i = 0;
    while let Some(c) = s[i..].chars().next() {
        match c {
            '`' => {
                if let Some(end) = s[i + 1..].find('`').filter(|e| *e > 0) {
                    flush(&mut text, out);
                    out.push(Piece::Code(unjoined(decode(&s[i + 1..i + 1 + end]), out)));
                    i += end + 2;
                    continue;
                }
            }
            '<' => {
                if let Some(end) = s[i + 1..].find(['>', '<'])
                    && s[i + 1 + end..].starts_with('>')
                    && let Some(p) = entity(&s[i + 1..i + 1 + end], marks)
                {
                    flush(&mut text, out);
                    let p = match p {
                        Piece::Link { url, label, marks } => {
                            let label = label.map(|l| unjoined(l, out)).filter(|l| !l.is_empty());
                            Piece::Link { url, label, marks }
                        }
                        Piece::Group(label) => Piece::Group(label.map(|l| unjoined(l, out))),
                        Piece::Text(t, m) => Piece::Text(unjoined(t, out), m),
                        other => other,
                    };
                    out.push(p);
                    i += end + 2;
                    continue;
                }
            }
            '*' | '_' | '~' => {
                if let Some(inner) = marks.with(c)
                    && opens(s, i)
                    && let Some(end) = closing(s, i, c)
                {
                    flush(&mut text, out);
                    inline(&s[i + 1..end], inner, out);
                    i = end + 1;
                    continue;
                }
            }
            _ => {}
        }
        text.push(c);
        i += c.len_utf8();
    }
    flush(&mut text, out);
}

/// `t` without the combining marks it starts with when pieces `before` it are on its line: with
/// the delimiter between them gone they would join the marks of the character before, past what
/// the sanitiser keeps on one character.
fn unjoined(t: String, before: &[Piece]) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    if before.is_empty() {
        return t;
    }
    let joined = format!("a{t}").graphemes(true).next().map_or(1, str::len) - 1;
    if joined == 0 { t } else { t[joined..].to_string() }
}

/// The delimiter at `i` of `s` may open a mark: after the start, a blank or punctuation, before
/// a non-blank.
fn opens(s: &str, i: usize) -> bool {
    let before = s[..i].chars().next_back();
    let after = s[i + 1..].chars().next();
    before.is_none_or(|b| !b.is_alphanumeric()) && after.is_some_and(|a| !a.is_whitespace())
}

/// Where the mark `c` opened at `i` of `s` closes: a `c` after a non-blank, before the end, a
/// blank or punctuation, with something between; code and `<…>` in between are passed over.
/// Looked for within [`REACH`] bytes.
fn closing(s: &str, i: usize, c: char) -> Option<usize> {
    // Never inside a character: a slice ends on a character's boundary.
    let mut end = s.len().min(i + 1 + REACH);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut j = i + 1;
    while j < end {
        let ch = s[j..].chars().next()?;
        match ch {
            '`' | '<' => {
                let close = if ch == '`' { '`' } else { '>' };
                match s[j + 1..end].find(close) {
                    Some(k) => j += k + 2,
                    None => j += 1,
                }
                continue;
            }
            _ if ch == c && j > i + 1 => {
                let before = s[..j].chars().next_back();
                let after = s[j + 1..].chars().next();
                if before.is_some_and(|b| !b.is_whitespace()) && after.is_none_or(|a| !a.is_alphanumeric()) {
                    return Some(j);
                }
            }
            _ => {}
        }
        j += ch.len_utf8();
    }
    None
}

/// The piece `<inner>` stands for; `None` when it is not one (it is then text).
fn entity(inner: &str, marks: Marks) -> Option<Piece> {
    let (what, label) = match inner.split_once('|') {
        Some((w, l)) => (w, Some(decode(l)).filter(|l| !l.is_empty())),
        None => (inner, None),
    };
    if what.is_empty() {
        return None;
    }
    Some(match what.as_bytes()[0] {
        b'@' => Piece::User { id: what[1..].to_string(), label, marks },
        b'#' => Piece::Channel { id: what[1..].to_string(), label, marks },
        b'!' => {
            let name = &what[1..];
            match name {
                "here" | "channel" | "everyone" => Piece::Broadcast(name.to_string()),
                _ if name.starts_with("subteam^") => Piece::Group(label),
                // A date and anything else Slack may add: the text it gives for it.
                _ => Piece::Text(label.unwrap_or_default(), marks),
            }
        }
        _ => Piece::Link { url: decode(what), label, marks },
    })
}

/// `s` with Slack's escapes decoded: `&amp;`, `&lt;` and `&gt;`, nothing else.
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// What a piece says as plain text, its people and channels named by `name`. A person not
/// known there shows the id (a label is the sender's word, and could name anyone); a channel
/// not known, the label Slack wrote, else the id. A link shows its label, and where it goes
/// too when the label reads as another address.
pub fn plain(piece: &Piece, name: &dyn Fn(&Piece) -> Option<String>) -> String {
    match piece {
        Piece::Text(t, _) | Piece::Code(t) => t.clone(),
        Piece::User { id, .. } => format!("@{}", name(piece).unwrap_or_else(|| id.clone())),
        Piece::Channel { id, label, .. } => {
            format!("#{}", name(piece).or_else(|| label.clone()).unwrap_or_else(|| id.clone()))
        }
        Piece::Broadcast(w) => format!("@{w}"),
        Piece::Group(label) => label.clone().unwrap_or_else(|| "@group".to_string()),
        Piece::Link { url, label: Some(label), .. } => match (host(label), host(url)) {
            (Some(said), Some(real)) if said != real => format!("{label} ({real})"),
            _ => label.clone(),
        },
        Piece::Link { url, label: None, .. } => shown_url(url).to_string(),
    }
}

/// The host `s` names when it reads as a web address (`https://a.example/x`, `a.example/x`),
/// in lower case, without its port.
fn host(s: &str) -> Option<String> {
    let lower = s.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).unwrap_or(&lower);
    let host = rest.split(['/', '?', '#']).next()?;
    let host =
        host.rsplit_once(':').map_or(host, |(h, port)| if port.bytes().all(|b| b.is_ascii_digit()) { h } else { host });
    let tld = host.rsplit_once('.')?.1;
    let shaped =
        host.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    (shaped && tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic())).then(|| host.to_string())
}

/// A link's address as shown when it has no label: a `mailto:` without its scheme.
/// (Never starting with a combining mark, which would join the character before it.)
pub fn shown_url(url: &str) -> &str {
    let url = url.strip_prefix("mailto:").unwrap_or(url);
    url.trim_start_matches(|c: char| unicode_width::UnicodeWidthChar::width(c) == Some(0))
}

/// The first line of `blocks` with text, as plain text (a tab title, a saved item's row).
pub fn first_line(blocks: &[Block], name: &dyn Fn(&Piece) -> Option<String>) -> Option<String> {
    blocks.iter().find_map(|b| {
        let text = match b {
            Block::Line { pieces, .. } => pieces.iter().map(|p| plain(p, name)).collect::<String>(),
            Block::Code(t) => t.clone(),
        };
        Some(text).filter(|t| !t.trim().is_empty())
    })
}

#[cfg(test)]
mod tests;
