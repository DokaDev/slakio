//! The UI side of the picture diagnostics ([`slakio_core::diag`], off unless asked for): words
//! for what the lines of [`super::Images`] and [`super::custom::Customs`] say.

use super::decode::Shape;
use slakio_core::backend::FailKind;
use slakio_core::diag::{Diag, Kind, Stage};
use slakio_core::images::Source;
use std::time::Instant;

/// One line about picture `key` (when on): its `id` and `shape`, then `pairs`.
pub(crate) fn line(diag: &Diag, key: &(Source, Shape), stage: Stage, result: &str, pairs: &[(&str, String)]) {
    if !diag.on() {
        return;
    }
    let id = diag.id(key.0.address());
    let mut all = vec![("id", id.as_str()), ("shape", shape_word(key.1))];
    all.extend(pairs.iter().map(|(k, v)| (*k, v.as_str())));
    diag.line(Kind::of(&key.0), stage, result, &all);
}

/// [`line`], the first time this run only.
pub(crate) fn once(diag: &Diag, key: &(Source, Shape), stage: Stage, result: &str, pairs: &[(&str, String)]) {
    if diag.on() && diag.first(Kind::of(&key.0), stage, &format!("{:?} {}", key.1, key.0.address())) {
        line(diag, key, stage, result, pairs);
    }
}

/// Picture `key` was read from `p.from` (width, height, format) into `p.bytes` sent, with its
/// frames (how many were read, whether it moves, why not).
pub(crate) fn decoded(diag: &Diag, key: &(Source, Shape), p: &super::Prepared) {
    if !diag.on() {
        return;
    }
    let (w, h, format) = p.from;
    let m = &p.moving;
    let animated = if m.frames.is_empty() { "no" } else { "yes" };
    let pairs = [
        ("w", w.to_string()),
        ("h", h.to_string()),
        ("format", format.to_string()),
        ("bytes", p.bytes.to_string()),
        ("frames", m.seen.to_string()),
        ("animated", animated.to_string()),
        ("why", if m.why.is_empty() { "none" } else { m.why }.to_string()),
    ];
    line(diag, key, Stage::Decoded, "ok", &pairs);
}

/// `kind` in a word (`network`, `cap-reached`, `api-file_not_available`).
pub(crate) fn fail_word(kind: &FailKind) -> String {
    match kind {
        FailKind::Api { code } => format!("api-{code}"),
        FailKind::RateLimited { .. } => "rate-limited".to_string(),
        FailKind::CapReached => "cap-reached".to_string(),
        FailKind::TooLarge => "too-large".to_string(),
        FailKind::ReadOnly => "read-only".to_string(),
        k => format!("{k:?}").to_ascii_lowercase(),
    }
}

/// When it is asked again: in whole seconds from `now`, or `never`.
pub(crate) fn retry(again: Option<Instant>, now: Instant) -> String {
    again.map_or_else(|| "never".to_string(), |at| at.saturating_duration_since(now).as_secs().to_string())
}

/// The cells a picture fills, in a word.
pub(crate) fn shape_word(shape: Shape) -> &'static str {
    match shape {
        Shape::Preview => "preview",
        Shape::Emoji => "emoji",
        s if s == super::CHIP_PHOTO => "chip",
        Shape::Photo { .. } => "photo",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_for_the_lines() {
        assert_eq!(fail_word(&FailKind::Api { code: "not_an_image".into() }), "api-not_an_image");
        assert_eq!(fail_word(&FailKind::Network), "network");
        assert_eq!(fail_word(&FailKind::RateLimited { retry_after: None }), "rate-limited");
        let now = Instant::now();
        assert_eq!(retry(None, now), "never");
        assert_eq!(retry(Some(now + std::time::Duration::from_secs(60)), now), "60");
        assert_eq!(shape_word(super::super::PHOTO), "photo");
        assert_eq!(shape_word(super::super::CHIP_PHOTO), "chip");
    }
}
