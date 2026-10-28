//! A message's Block Kit blocks and legacy attachments as Slack markup (mrkdwn) text, so the UI
//! reads every message one way. People write rich text (`rich_text` blocks), which Slack also
//! sends as the message's `text`; bots post sections, headers, fields and context, or
//! attachments, with a `text` that is only a notification's fallback. So:
//!
//! - a message whose blocks are rich text only is its `text` (the same words), unless that is
//!   empty: then its blocks are written out;
//! - any other block (a section, header, context…) is written out in place of the fallback;
//! - attachments are added after the text, and the names of the files shared after them.
//!
//! What is written is text as Slack would write it: `&`, `<` and `>` escaped, styles as marks,
//! people, channels and links as `<…>`. Unknown kinds of blocks and elements are left out (their
//! words, when they have some, are kept). Every text it makes is remote text, sanitised before
//! it is drawn.

use serde_json::Value;

/// How deep blocks are read inside one another (a list in a quote in a list…).
const DEPTH: usize = 8;

/// The text to show for a message of `text`, `blocks`, `attachments` and the names of `files`.
pub fn text(text: &str, blocks: Option<&Value>, attachments: Option<&Value>, files: &[String]) -> String {
    let blocks = blocks.and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let rich_only = blocks.iter().all(|b| kind(b) == "rich_text");
    let mut parts: Vec<String> = Vec::new();
    if rich_only && !text.trim().is_empty() {
        parts.push(text.to_string());
    } else if !blocks.is_empty() {
        let written: Vec<String> = blocks.iter().filter_map(block).filter(|s| !s.trim().is_empty()).collect();
        if written.is_empty() { parts.push(text.to_string()) } else { parts.push(written.join("\n")) }
    } else {
        parts.push(text.to_string());
    }
    for a in attachments.and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
        parts.push(attachment(a));
    }
    parts.extend(files.iter().map(|f| format!("📎 {}", escape(f))));
    parts.retain(|p| !p.trim().is_empty());
    parts.join("\n")
}

fn kind(v: &Value) -> &str {
    v.get("type").and_then(Value::as_str).unwrap_or_default()
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// A composition text object (`{"type": "mrkdwn" | "plain_text", "text": …}`) as mrkdwn.
fn text_object(v: &Value) -> Option<String> {
    let t = str_of(v, "text")?;
    Some(if kind(v) == "plain_text" { escape(t) } else { t.to_string() })
}

/// One block as text.
fn block(b: &Value) -> Option<String> {
    match kind(b) {
        "rich_text" => Some(rich(b.get("elements")?, 0)),
        "section" => {
            let mut lines: Vec<String> = b.get("text").and_then(text_object).into_iter().collect();
            if let Some(fields) = b.get("fields").and_then(Value::as_array) {
                lines.extend(fields.iter().filter_map(text_object));
            }
            Some(lines.join("\n"))
        }
        "header" => b.get("text").and_then(text_object).map(|t| format!("*{}*", t.trim())),
        "context" => {
            let words: Vec<String> =
                b.get("elements")?.as_array()?.iter().filter_map(|e| text_object(e).or_else(|| alt(e))).collect();
            Some(words.join(" "))
        }
        "image" => alt(b),
        _ => None,
    }
}

/// An image's description, if it has one.
fn alt(v: &Value) -> Option<String> {
    str_of(v, "alt_text").filter(|a| !a.trim().is_empty()).map(|a| format!("🖼 {}", escape(a)))
}

/// Rich text elements (`rich_text_section`, `_list`, `_quote`, `_preformatted`) as lines.
fn rich(elements: &Value, depth: usize) -> String {
    if depth > DEPTH {
        return String::new();
    }
    let mut out: Vec<String> = Vec::new();
    for e in elements.as_array().map(Vec::as_slice).unwrap_or_default() {
        let inner = e.get("elements").cloned().unwrap_or(Value::Null);
        match kind(e) {
            "rich_text_section" => out.push(inline(&inner)),
            "rich_text_preformatted" => out.push(format!("```\n{}\n```", inline(&inner))),
            "rich_text_quote" => {
                out.extend(inline(&inner).split('\n').map(|l| format!("&gt; {l}")));
            }
            "rich_text_list" => {
                let ordered = str_of(e, "style") == Some("ordered");
                let indent = e.get("indent").and_then(Value::as_u64).unwrap_or(0).min(6) as usize;
                for (n, item) in inner.as_array().map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
                    let mark = if ordered { format!("{}.", n + 1) } else { "•".to_string() };
                    let body = item.get("elements").map(inline).unwrap_or_default();
                    out.push(format!("{}{mark} {body}", "    ".repeat(indent)));
                }
            }
            _ => out.push(rich(&inner, depth + 1)),
        }
    }
    out.retain(|l| !l.is_empty());
    out.join("\n")
}

/// The inline elements of a rich text section as mrkdwn.
fn inline(elements: &Value) -> String {
    let mut out = String::new();
    for e in elements.as_array().map(Vec::as_slice).unwrap_or_default() {
        let piece = match kind(e) {
            "text" => styled(&escape(str_of(e, "text").unwrap_or_default()), e.get("style")),
            "link" => {
                let url = str_of(e, "url").unwrap_or_default();
                let link = match str_of(e, "text").filter(|t| !t.is_empty()) {
                    Some(t) => format!("<{}|{}>", escape_url(url), escape(t)),
                    None => format!("<{}>", escape_url(url)),
                };
                styled(&link, e.get("style"))
            }
            "user" => format!("<@{}>", id(e, "user_id")),
            "channel" => format!("<#{}>", id(e, "channel_id")),
            "usergroup" => format!("<!subteam^{}>", id(e, "usergroup_id")),
            "broadcast" => format!("<!{}>", id(e, "range")),
            "emoji" => format!(":{}:", id(e, "name")),
            "date" => escape(str_of(e, "fallback").unwrap_or_default()),
            _ => escape(str_of(e, "text").unwrap_or_default()),
        };
        out.push_str(&piece);
    }
    out
}

/// An id-like field: letters, digits and `_+-^.` only, so it cannot end the `<…>` it is in.
fn id(e: &Value, key: &str) -> String {
    str_of(e, key).unwrap_or_default().chars().filter(|c| c.is_alphanumeric() || "_+-^.'".contains(*c)).collect()
}

/// `text` in the marks `style` turns on (`{"bold": true, "code": true, …}`); blanks around it
/// stay outside the marks, so they open and close where Slack reads them.
fn styled(text: &str, style: Option<&Value>) -> String {
    let on = |k: &str| style.and_then(|s| s.get(k)).and_then(Value::as_bool) == Some(true);
    let core = text.trim();
    if core.is_empty() {
        return text.to_string();
    }
    let mut marked = core.to_string();
    for (k, m) in [("code", "`"), ("strike", "~"), ("italic", "_"), ("bold", "*")] {
        if on(k) {
            marked = format!("{m}{marked}{m}");
        }
    }
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    format!("{}{marked}{}", &text[..start], &text[end..])
}

/// A legacy attachment as lines: its pretext, title, text and fields, else its fallback.
fn attachment(a: &Value) -> String {
    let mut lines: Vec<String> = Vec::new();
    for key in ["pretext", "author_name"] {
        lines.extend(str_of(a, key).map(str::to_string));
    }
    if let Some(title) = str_of(a, "title") {
        lines.push(match str_of(a, "title_link") {
            Some(link) => format!("*<{}|{}>*", escape_url(link), escape(title)),
            None => format!("*{}*", escape(title)),
        });
    }
    lines.extend(str_of(a, "text").map(str::to_string));
    for f in a.get("fields").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
        let (title, value) = (str_of(f, "title").unwrap_or_default(), str_of(f, "value").unwrap_or_default());
        lines.push(if title.is_empty() { value.to_string() } else { format!("*{}*\n{value}", escape(title)) });
    }
    if let Some(blocks) = a.get("blocks").and_then(Value::as_array) {
        lines.extend(blocks.iter().filter_map(block));
    }
    lines.retain(|l| !l.trim().is_empty());
    if lines.is_empty() {
        lines.extend(str_of(a, "fallback").map(str::to_string));
    }
    lines.join("\n")
}

/// Plain text as mrkdwn: `&`, `<`, `>` escaped.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// An address inside `<…>`: escaped, and no `|` (it would end the address).
fn escape_url(s: &str) -> String {
    escape(s).replace('|', "%7C")
}

#[cfg(test)]
mod tests;
