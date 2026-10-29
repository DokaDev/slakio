//! What of a fault's detail may reach the error log: never a token, a cookie, a request's
//! address past its host, or text someone wrote (quoted in a parser's words). Whatever is left
//! out says so (`[secret]`, `…`), and a line is never longer than [`LINE_MAX`] characters.

/// The most characters of one detail kept.
pub const LINE_MAX: usize = 300;

/// A run of characters this long with no path separator in it is read as a secret: an unknown
/// token or cookie shape.
const OPAQUE_MIN: usize = 32;

/// The names whose value (`name=value`, `name: value`) is a secret, lowercase.
const SECRET_NAMES: &[&str] = &["token", "d", "d-s", "cookie", "authorization", "password", "secret", "session"];

/// `detail` as the error log may keep it: one line, secrets and remote text left out.
pub fn redact(detail: &str) -> String {
    let line = unquoted(&detail.replace(['\n', '\r'], " | "));
    let mut out: Vec<String> = Vec::new();
    let mut rest_secret = false;
    for word in line.split(' ') {
        if rest_secret {
            continue;
        }
        let lower = word.to_ascii_lowercase();
        // A header that carries the sign-in: everything after it is.
        if lower.ends_with("cookie:") || lower.ends_with("authorization:") {
            out.push(format!("{word} [secret]"));
            rest_secret = true;
            continue;
        }
        out.push(redact_word(word));
    }
    let text = out.join(" ");
    match text.char_indices().nth(LINE_MAX) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text,
    }
}

/// One word (no spaces) of a detail.
fn redact_word(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if is_token(&lower) {
        return "[secret]".to_string();
    }
    if let Some(at) = lower.find("://") {
        return address(word, at);
    }
    // A long run of base64 or percent-encoded characters (`=` padding and `/` too) is opaque,
    // whatever it holds; a path (it starts with `/` or `~`) is not.
    let bare = word.trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | ',' | '.' | ':' | '<' | '>'));
    let body = bare.trim_end_matches('=');
    let opaque = body.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/_-%".contains(&b));
    if opaque && body.len() >= OPAQUE_MIN && !bare.starts_with(['/', '~']) {
        return "[secret]".to_string();
    }
    // `name=value` pairs (also `a=1;d=…` and `name=value&…`): a secret name's value goes.
    if word.contains('=') {
        let pairs: Vec<String> = word.split(['&', ';', ',']).map(pair).collect();
        let seps: Vec<char> = word.chars().filter(|c| matches!(c, '&' | ';' | ',')).collect();
        let mut joined = String::new();
        for (i, p) in pairs.iter().enumerate() {
            joined.push_str(p);
            if let Some(s) = seps.get(i) {
                joined.push(*s);
            }
        }
        return joined;
    }
    let bare = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if bare.chars().count() >= OPAQUE_MIN && !bare.contains(['/', '\\']) {
        return "[secret]".to_string();
    }
    word.to_string()
}

/// `name=value`, the value left out when the name is a secret's (or the value looks like one).
fn pair(p: &str) -> String {
    let Some((name, value)) = p.split_once('=') else { return p.to_string() };
    // The name after a path or another pair (`/api/x?password=…`).
    let name_only = name.rsplit(['?', '/', '&', ';']).next().unwrap_or(name);
    let key = name_only.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-').to_ascii_lowercase();
    if SECRET_NAMES.contains(&key.as_str()) || is_token(&value.to_ascii_lowercase()) {
        return format!("{name}=[secret]");
    }
    if value.chars().count() >= OPAQUE_MIN {
        return format!("{name}=[secret]");
    }
    p.to_string()
}

/// A Slack token or cookie (`xox`, a letter and `-`, or `xapp-`, then the rest) anywhere in
/// `lower`.
fn is_token(lower: &str) -> bool {
    let b = lower.as_bytes();
    let xox = (0..b.len().saturating_sub(4))
        .any(|i| &b[i..i + 3] == b"xox" && b[i + 3].is_ascii_alphabetic() && b.get(i + 4) == Some(&b'-'));
    xox || lower.contains("xapp-")
}

/// An address (`scheme://…` from `at`): its scheme and host only, the rest (path, query, a
/// sign-in in it) left out.
fn address(word: &str, at: usize) -> String {
    let (head, rest) = word.split_at(at + 3);
    // What comes before the scheme in the same word (`session=…;next=https://…`) is a word too.
    let scheme_at =
        head[..at].rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))).map_or(0, |i| i + 1);
    let before = &head[..scheme_at];
    let head = format!("{}{}", if before.is_empty() { String::new() } else { redact_word(before) }, &head[scheme_at..]);
    let end = rest.find(['/', '?', '#', '\'', '"']).unwrap_or(rest.len());
    let host = &rest[..end];
    // `user:password@host`: only the host.
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host: String = host.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':')).collect();
    if end < rest.len() { format!("{head}{host}/…") } else { format!("{head}{host}") }
}

/// `text` with whatever is between quotes (`"…"`, `'…'`, `` `…` ``) left out: a parser quotes
/// the text it could not read, which may be someone's words.
fn unquoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut open: Option<char> = None;
    for c in text.chars() {
        match open {
            Some(q) if c == q => {
                out.push('…');
                out.push(c);
                open = None;
            }
            Some(_) => {}
            None if matches!(c, '"' | '`') => {
                out.push(c);
                open = Some(c);
            }
            // An apostrophe inside a word (`don't`) does not open a quote.
            None if c == '\'' && !out.ends_with(|p: char| p.is_alphanumeric()) => {
                out.push(c);
                open = Some(c);
            }
            None => out.push(c),
        }
    }
    // A quote that is never closed: what follows it is left out too.
    if open.is_some() {
        out.push('…');
    }
    out
}
