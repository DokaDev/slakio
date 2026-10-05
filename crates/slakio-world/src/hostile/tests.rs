use super::*;

#[test]
fn the_corpus_covers_every_family_the_sanitiser_must_stop() {
    let all = all();
    let any = |p: &dyn Fn(&str) -> bool| all.iter().any(|h| p(&h.text));
    assert!(any(&|t| t.contains("\x1b[")), "7-bit CSI");
    assert!(any(&|t| t.contains("\x1b]")), "7-bit OSC");
    assert!(any(&|t| t.contains("\x1bP")), "DCS");
    assert!(any(&|t| t.contains("\x1b_")), "APC");
    assert!(any(&|t| t.contains('\u{9b}') && t.contains('\u{9d}')), "8-bit C1");
    assert!(any(&|t| t.contains('\u{202E}')), "bidi override");
    assert!(any(&|t| t.matches('\u{200B}').count() >= 1000), "zero-width flood");
    assert!(any(&|t| t.chars().filter(|c| ('\u{300}'..='\u{36F}').contains(c)).count() >= 500), "combining flood");
    assert!(any(&|t| t.chars().any(|c| c.is_control() && c != '\x1b')), "C0 controls");
    assert!(any(&|t| t.len() >= 100 * 1024 && !t.contains(' ')), "100 kB line");
    assert!(any(&|t| t.contains('\u{FFFD}')), "malformed UTF-8 replaced on decode");
}

#[test]
fn every_entry_says_what_it_attacks_once() {
    let all = all();
    for (i, h) in all.iter().enumerate() {
        assert!(!h.what.is_empty() && !h.text.is_empty());
        assert!(!all[i + 1..].iter().any(|o| o.what == h.what), "{} twice", h.what);
    }
}
