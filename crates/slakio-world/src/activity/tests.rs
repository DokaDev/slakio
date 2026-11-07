use super::*;

#[test]
fn every_kind_is_there_newest_first_with_many_pages() {
    let w = World::demo();
    let a = WorkspaceId::new("TDEMOA");
    let all = w.activity(&a);
    assert!(all.len() > 200, "many pages: {}", all.len());
    let first: Vec<&str> = all.iter().take(20).map(|n| n.kind).collect();
    for kind in ["thread_v2", "dm", "message_reaction"] {
        assert!(first.contains(&kind), "{kind} on the first page: {first:?}");
    }
    assert!(all.windows(2).all(|p| p[0].at >= p[1].at), "newest first");
    for kind in ["at_user", "at_channel", "at_user_group", "keyword", "message_reaction", "dm", "thread_v2"] {
        assert!(all.iter().any(|n| n.kind == kind), "{kind}");
    }
    let thread = all.iter().find(|n| n.kind == "thread_v2").unwrap();
    assert!(thread.thread.is_some_and(|t| t < thread.message.ts), "the newest reply of a thread");
    let hostile = w.find("TDEMOA", names::HOSTILE).unwrap();
    assert!(all.iter().any(|n| n.conversation == hostile.id));
    let mut keys: Vec<&str> = all.iter().map(|n| n.key.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), all.len(), "each key once");
    let b = w.activity(&WorkspaceId::new("TDEMOB"));
    assert!(!b.is_empty() && b.iter().all(|n| n.conversation.as_str().starts_with(['C', 'D', 'G'])));
    assert!(w.activity(&WorkspaceId::new("TNONE")).is_empty());
    assert_eq!(w.activity(&a), World::demo().activity(&a), "the same every run");
}
