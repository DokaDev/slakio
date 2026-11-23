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

/// `path` is one of an emoji's picture's address ([`picture`]): `/<team>/<name>/<hex>.png`.
pub fn is_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    matches!(parts.as_slice(), ["", team, name, file]
        if team.starts_with('T') && team.bytes().all(|b| b.is_ascii_alphanumeric())
            && slakio_core::images::emoji::shortcode(name)
            && file.strip_suffix(".png").is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit())))
}

#[cfg(test)]
mod tests;
