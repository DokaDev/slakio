//! Invented message text: English-like sentences, some Korean (written with `\u{…}` escapes,
//! since source files hold no Hangul), emoji including ZWJ sequences, code and links. Nothing
//! here comes from a real workspace.

use crate::rng::Rng;

const WORDS: &[&str] = &[
    "deploy",
    "rollback",
    "latency",
    "review",
    "merge",
    "release",
    "build",
    "queue",
    "cache",
    "index",
    "migration",
    "dashboard",
    "alert",
    "pager",
    "staging",
    "production",
    "branch",
    "ticket",
    "meeting",
    "design",
    "draft",
    "schema",
    "timeout",
    "retry",
    "metrics",
    "tracing",
    "budget",
    "roadmap",
    "sprint",
    "customer",
    "invoice",
    "the",
    "a",
    "is",
    "looks",
    "fine",
    "broken",
    "again",
    "today",
    "tomorrow",
    "after",
    "lunch",
    "please",
    "check",
    "can",
    "someone",
    "we",
    "should",
    "maybe",
    "done",
    "now",
    "still",
    "waiting",
    "for",
    "on",
];

const OPENERS: &[&str] = &[
    "Starting deploy",
    "PR is up",
    "Confirmed",
    "Looking into it",
    "Suspect the DB pool",
    "Rolling back",
    "Ok, monitoring",
    "Can someone review this?",
    "Meeting moved to 3pm",
    "Thanks!",
];

/// Korean phrases, as escapes (repository rule: no Hangul in source files).
const KOREAN: &[&str] = &[
    // "Hello"
    "\u{C548}\u{B155}\u{D558}\u{C138}\u{C694}",
    // "Checked the deploy"
    "\u{BC30}\u{D3EC} \u{D655}\u{C778}\u{D588}\u{C2B5}\u{B2C8}\u{B2E4}",
    // "Shall we have a meeting?"
    "\u{D68C}\u{C758} \u{D560}\u{AE4C}\u{C694}?",
    // "Review please"
    "\u{B9AC}\u{BDF0} \u{BD80}\u{D0C1}\u{B4DC}\u{B824}\u{C694}",
];

const EMOJI: &[&str] = &[
    "\u{1F44D}",
    "\u{1F440}",
    "\u{1F389}",
    // woman technologist (ZWJ)
    "\u{1F469}\u{200D}\u{1F4BB}",
    // family (ZWJ)
    "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}",
    // thumbs up, medium skin tone
    "\u{1F44D}\u{1F3FD}",
    // flag
    "\u{1F1F0}\u{1F1F7}",
];

/// One message's text.
pub fn message(r: &mut Rng) -> String {
    match r.below(100) {
        0..=7 => r.pick(KOREAN).to_string(),
        8..=11 => format!("{} {}", r.pick(OPENERS), r.pick(EMOJI)),
        12..=13 => "```\nSELECT count(*) FROM jobs WHERE state = 'stuck';\n```".to_string(),
        14..=15 => "see https://docs.example.com/runbooks/latency".to_string(),
        16..=30 => r.pick(OPENERS).to_string(),
        _ => sentence(r),
    }
}

fn sentence(r: &mut Rng) -> String {
    let n = 3 + r.below(14) as usize;
    let mut s = String::new();
    for i in 0..n {
        if i > 0 {
            s.push(' ');
        }
        let word: &&str = r.pick(WORDS);
        s.push_str(word);
    }
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => s,
    }
}

/// A word for generated channel names.
pub fn topic(r: &mut Rng) -> &'static str {
    let word: &&str = r.pick(&WORDS[..31]);
    word
}
