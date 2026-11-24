//! The world's own emoji, as Slack's `emoji.list` names them: a few invented pictures on the
//! emoji host's name ([`EMOJI_HOSTS`], so they pass the same checks as Slack's; the fake serves
//! them from its loopback address, [`crate::pictures::served`]), an alias of one, an alias of a
//! standard emoji, a cycle and pictures elsewhere (which must draw nothing of their own).

use slakio_core::images::EMOJI_HOSTS;

/// The side of an emoji's picture, in pixels.
pub const SIDE: u32 = 64;

/// The address of workspace `team`'s emoji `name`'s picture.
pub fn picture(team: &str, name: &str) -> String {
    format!("https://{}/{team}/{name}/{:08x}.png", EMOJI_HOSTS[0], crate::pictures::hash(name))
}

/// Workspace `team`'s own emoji, `name → address or alias:other`, in `emoji.list`'s words.
pub fn list(team: &str) -> Vec<(String, String)> {
    [
        ("party-blob", picture(team, "party-blob")),
        ("ship-it", picture(team, "ship-it")),
        ("blob-dance", "alias:party-blob".to_string()),
        ("yes", "alias:white_check_mark".to_string()),
        ("loop-a", "alias:loop-b".to_string()),
        ("loop-b", "alias:loop-a".to_string()),
        ("far-away", "https://evil.example/emoji/far-away.png".to_string()),
        ("plain", format!("http://{}/{team}/plain/0.png", EMOJI_HOSTS[0])),
    ]
    .into_iter()
    .map(|(n, v)| (n.to_string(), v))
    .collect()
}

/// `path` is one of an emoji's picture's address ([`picture`]): `/<team>/<name>/<hex>.png`, the
/// name read percent-decoded (as a request names one of another script).
pub fn is_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    matches!(parts.as_slice(), ["", team, name, file]
        if team.starts_with('T') && team.bytes().all(|b| b.is_ascii_alphanumeric())
            && decoded(name).is_some_and(|n| slakio_core::images::emoji::shortcode(&n))
            && file.strip_suffix(".png").is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit())))
}

/// `s` with its `%XX` escapes decoded (a name of another script comes encoded); `None` when an
/// escape is broken or the result is not UTF-8.
fn decoded(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests;
