//! Slack's emoji shortcodes as Unicode emoji: `:+1:`, `:white_check_mark:`, `:eyes:` and the
//! skin tones Slack writes after them (`:+1::skin-tone-3:`, a reaction named `+1::skin-tone-3`),
//! from the gemoji names of the `emojis` crate and the few aliases Slack adds. A name it does not
//! know — a workspace's custom emoji, until their images come — stays `:name:` text.
//!
//! The text given is sanitised already; an emoji is never a control or format character, so
//! what comes out is as safe. Widths follow grapheme clusters ([`crate::text::width`]): an emoji,
//! a skin-toned one and a ZWJ sequence are two cells, like a CJK character.

use emojis::SkinTone;

/// Slack's names that gemoji spells otherwise.
const ALIASES: &[(&str, &str)] =
    &[("simple_smile", "slightly_smiling_face"), ("thumbsup_all", "+1"), ("facepunch", "punch")];

/// The skin tone `skin-tone-2` … `skin-tone-6` names.
fn tone(name: &str) -> Option<SkinTone> {
    match name.strip_prefix("skin-tone-")? {
        "2" => Some(SkinTone::Light),
        "3" => Some(SkinTone::MediumLight),
        "4" => Some(SkinTone::Medium),
        "5" => Some(SkinTone::MediumDark),
        "6" => Some(SkinTone::Dark),
        _ => None,
    }
}

/// The emoji of shortcode `name` (no colons; `name::skin-tone-N` too), if it is a standard one.
pub fn get(name: &str) -> Option<&'static str> {
    let (base, skin) = match name.split_once("::") {
        Some((b, s)) => (b, tone(s)),
        None => (name, None),
    };
    let base = ALIASES.iter().find(|(a, _)| *a == base).map_or(base, |(_, to)| *to);
    let e = emojis::get_by_shortcode(base)?;
    Some(skin.and_then(|s| e.with_skin_tone(s)).unwrap_or(e).as_str())
}

/// `text` with every `:name:` (and a `:skin-tone-N:` right after it) of a standard emoji drawn
/// as the emoji; anything else is left as it is.
pub fn replace(text: &str) -> String {
    if !text.contains(':') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(':') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find(':');
        let name = end.map(|e| &after[..e]).filter(|n| valid(n));
        match name.and_then(get) {
            Some(mut emoji) => {
                let mut next = &after[name.unwrap_or_default().len() + 1..];
                // `:+1::skin-tone-3:`: the tone follows as a shortcode of its own.
                if let Some(tail) = next.strip_prefix(':')
                    && let Some(e) = tail.find(':')
                    && tone(&tail[..e]).is_some()
                    && let Some(toned) = get(&format!("{}::{}", name.unwrap_or_default(), &tail[..e]))
                {
                    emoji = toned;
                    next = &tail[e + 1..];
                }
                out.push_str(emoji);
                rest = next;
            }
            None => {
                out.push(':');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A shortcode: letters, digits and `_ + - '`, as Slack names them.
fn valid(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || "_+-'".contains(c))
}

#[cfg(test)]
mod tests;
