use super::*;

#[test]
fn a_query_reads_words_quotes_and_filters() {
    let q = parse(r#"  Deploy in:#Backend from:@alex "went OUT" after:2026-01-01 "#);
    assert_eq!(q.words, ["deploy", "went out"]);
    assert_eq!(q.within.as_deref(), Some("backend"));
    assert_eq!(q.from.as_deref(), Some("alex"));
    assert_eq!(parse("\"in:#x\"").words, ["in:#x"], "a quoted filter is a word");
    assert_eq!(parse("\"open").words, ["open"]);
    assert_eq!(parse("").words, Vec::<String>::new());
}

#[test]
fn words_are_marked_and_a_message_without_one_is_not_found() {
    let marked = mark("The Deploy went out", &["deploy".into(), "out".into()]).unwrap();
    assert_eq!(marked, "The \u{E000}Deploy\u{E001} went \u{E000}out\u{E001}");
    assert_eq!(mark("nothing here", &["deploy".into()]), None);
}

#[test]
fn the_world_finds_newest_first_within_a_conversation_and_in_threads() {
    let w = World::demo();
    let ws = w.snapshot().workspaces[0].id.clone();
    let all = w.search(&ws, "the");
    assert!(all.len() > 100, "{}", all.len());
    assert!(all.windows(2).all(|p| p[0].message.ts >= p[1].message.ts), "newest first");
    assert!(all.iter().all(|h| h.message.text.unsanitized().contains(MATCH_START)));
    assert!(all.iter().any(|h| h.thread.is_some()), "replies are found too");
    let big = w.find(ws.as_str(), names::BIG_HISTORY).unwrap().id.clone();
    let within = w.search(&ws, &format!("the in:#{}", names::BIG_HISTORY));
    assert!(!within.is_empty() && within.iter().all(|h| h.conversation == big));
    assert!(w.search(&ws, "").is_empty());
    assert!(w.search(&ws, "zzqqxxnothing").is_empty());
}
