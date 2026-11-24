//! Slack's emoji shortcodes as Unicode emoji: `:+1:`, `:white_check_mark:`, `:eyes:` and the
//! skin tones Slack writes after them (`:+1::skin-tone-3:`, a reaction named `+1::skin-tone-3`),
//! from the gemoji names of the `emojis` crate and the common names Slack spells otherwise. A
//! name it does not know — a workspace's own emoji — stays `:name:` text here; the timeline draws
//! those where it can (`ui::timeline::custom`).
//!
//! The text given is sanitised already; an emoji is never a control or format character, so
//! what comes out is as safe. Widths follow grapheme clusters ([`crate::text::width`]): an emoji,
//! a skin-toned one and a ZWJ sequence are two cells, like a CJK character.

use emojis::SkinTone;
use slakio_core::images::emoji::shortcode;

/// Slack's names that gemoji spells otherwise (the common ones).
const ALIASES: &[(&str, &str)] = &[
    ("simple_smile", "slightly_smiling_face"),
    ("thumbsup_all", "+1"),
    ("facepunch", "punch"),
    ("thinking_face", "thinking"),
    ("face_with_rolling_eyes", "roll_eyes"),
    ("hugging_face", "hugs"),
    ("star-struck", "star_struck"),
    ("grinning_face_with_star_eyes", "star_struck"),
    ("face_with_hand_over_mouth", "hand_over_mouth"),
    ("face_with_monocle", "monocle_face"),
    ("face_with_raised_eyebrow", "raised_eyebrow"),
    ("smiling_face_with_3_hearts", "smiling_face_with_three_hearts"),
    ("rolling_on_the_floor_laughing", "rofl"),
    ("white_frowning_face", "frowning_face"),
    ("face_with_cowboy_hat", "cowboy_hat_face"),
    ("robot_face", "robot"),
    ("face_vomiting", "vomiting_face"),
    ("face_with_symbols_on_mouth", "cursing_face"),
    ("face_palm", "facepalm"),
    ("man-shrugging", "man_shrugging"),
    ("woman-shrugging", "woman_shrugging"),
    ("man-facepalming", "man_facepalming"),
    ("woman-facepalming", "woman_facepalming"),
    ("man-raising-hand", "raising_hand_man"),
    ("woman-raising-hand", "raising_hand_woman"),
    ("man-bowing", "bowing_man"),
    ("woman-bowing", "bowing_woman"),
    ("male-technologist", "man_technologist"),
    ("female-technologist", "woman_technologist"),
    ("the_horns", "metal"),
    ("spock-hand", "vulcan_salute"),
    ("hand_with_index_and_middle_fingers_crossed", "crossed_fingers"),
    ("heavy_heart_exclamation_mark_ornament", "heavy_heart_exclamation"),
    ("sports_medal", "medal_sports"),
    ("first_place_medal", "1st_place_medal"),
    ("spiral_calendar_pad", "spiral_calendar"),
    ("spiral_note_pad", "spiral_notepad"),
    ("lower_left_ballpoint_pen", "pen"),
    ("linked_paperclips", "paperclips"),
    ("mostly_sunny", "sun_behind_small_cloud"),
    ("partly_sunny_rain", "sun_behind_rain_cloud"),
    ("rain_cloud", "cloud_with_rain"),
    ("flag-us", "us"),
    ("flag-gb", "gb"),
    ("flag-kr", "kr"),
    ("flag-jp", "jp"),
];

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
        let name = end.map(|e| &after[..e]).filter(|n| shortcode(n));
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

#[cfg(test)]
mod tests;
