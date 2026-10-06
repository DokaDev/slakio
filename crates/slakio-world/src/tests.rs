use super::*;
use std::collections::HashSet;

fn world() -> World {
    World::demo()
}

fn conv<'a>(w: &'a World, name: &str) -> &'a Conversation {
    w.find("TDEMOA", name).unwrap_or_else(|| panic!("{name}"))
}

#[test]
fn the_same_seed_gives_the_same_world() {
    let (a, b) = (World::new(5), World::new(5));
    assert_eq!(a.snapshot(), b.snapshot());
    let c = &a.snapshot().conversations[7].id;
    for i in 0..a.message_count(c) {
        assert_eq!(a.message(c, i), b.message(c, i));
    }
    let other = World::new(6);
    let unread = |w: &World| w.snapshot().conversations.iter().map(|c| c.unread).collect::<Vec<_>>();
    assert_ne!(unread(&a), unread(&other), "another seed, another world");
}

#[test]
fn two_workspaces_with_sections_about_300_channels_and_dms() {
    let w = world();
    let s = w.snapshot();
    let names: Vec<_> = s.workspaces.iter().map(|w| w.name.unsanitized()).collect();
    assert_eq!(names, ["A company", "B side"]);
    assert_ne!(s.workspaces[0].color, s.workspaces[1].color);
    let channels = s.conversations.iter().filter(|c| !c.is_dm()).count();
    assert!((280..=320).contains(&channels), "{channels} channels");
    for ws in &s.workspaces {
        let dms = s.conversations.iter().filter(|c| c.workspace == ws.id && c.is_dm()).count();
        assert!(dms >= 5, "{:?}: {dms} DMs", ws.name);
        assert!(w.me(&ws.id).is_some());
    }
    assert!(s.conversations.iter().any(|c| matches!(c.kind, ConversationKind::GroupDm { .. })));
    assert!(s.conversations.iter().any(|c| c.muted), "some muted channels");
    assert!(s.conversations.iter().any(|c| c.mentions > 0), "some mentions");
    assert!(s.users.iter().any(|u| u.bot), "bots");
}

#[test]
fn ids_are_unique_and_every_reference_resolves() {
    let w = world();
    let s = w.snapshot();
    let mut ids = HashSet::new();
    for c in &s.conversations {
        assert!(ids.insert(c.id.as_str()), "{} twice", c.id);
        let section = s.sections.iter().find(|x| x.id == c.section).expect("section exists");
        assert_eq!(section.workspace, c.workspace, "{:?}", c.name);
        assert!(c.mentions <= c.unread, "{:?}", c.name);
        if let ConversationKind::Dm { user } = &c.kind {
            assert!(s.users.iter().any(|u| &u.id == user));
        }
    }
    let users: HashSet<_> = s.users.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(users.len(), s.users.len(), "user ids unique");
    for c in &s.conversations {
        for i in 0..w.message_count(&c.id).min(30) {
            let m = w.message(&c.id, i).unwrap();
            assert!(users.contains(m.user.as_str()), "{:?}: poster {}", c.name, m.user);
        }
    }
}

#[test]
fn the_slack_connect_channel_has_people_of_another_organization() {
    let w = world();
    let shared = conv(&w, names::SHARED);
    assert!(shared.external);
    let external: HashSet<_> =
        w.snapshot().users.iter().filter(|u| matches!(u.org, Org::External(_))).map(|u| u.id.clone()).collect();
    assert!(!external.is_empty());
    let posters: HashSet<_> =
        (0..w.message_count(&shared.id)).map(|i| w.message(&shared.id, i).unwrap().user).collect();
    assert!(posters.iter().any(|p| external.contains(p)), "people of the other organization post there");
    assert!(w.snapshot().conversations.iter().filter(|c| c.external).count() == 1);
}

#[test]
fn dm_peers_are_active_away_or_in_do_not_disturb() {
    let w = world();
    let snap = w.snapshot();
    let peers: Vec<Presence> = snap
        .conversations
        .iter()
        .filter_map(|c| match &c.kind {
            ConversationKind::Dm { user } => snap.users.iter().find(|u| &u.id == user).map(|u| u.presence),
            _ => None,
        })
        .collect();
    for p in [Presence::Active, Presence::Away, Presence::Dnd] {
        assert!(peers.contains(&p), "a DM peer is {p:?}: {peers:?}");
    }
    assert!(!peers.contains(&Presence::Unknown), "the demo knows everyone's presence");
}

#[test]
fn the_big_channel_has_10k_messages_in_time_order() {
    let w = world();
    let big = &conv(&w, names::BIG_HISTORY).id;
    assert_eq!(w.message_count(big), BIG_HISTORY_MESSAGES);
    let mut last = Ts(0);
    for i in 0..BIG_HISTORY_MESSAGES {
        let m = w.message(big, i).unwrap();
        assert!(m.ts > last, "message {i} is not after the one before");
        last = m.ts;
    }
    assert_eq!(w.message(big, BIG_HISTORY_MESSAGES), None);
    assert_eq!(w.message(&ConversationId::new("CNOPE"), 0), None);
    assert_eq!(w.message_count(&ConversationId::new("CNOPE")), 0);
}

#[test]
fn the_long_thread_has_1200_replies_in_time_order() {
    let w = world();
    let c = &conv(&w, names::LONG_THREADS).id;
    let root = w.message(c, 0).unwrap();
    let thread = root.thread.unwrap();
    assert_eq!(thread.replies, LONG_THREAD_REPLIES);
    let mut last = root.ts;
    for i in 0..LONG_THREAD_REPLIES {
        let r = w.reply(c, 0, i).unwrap();
        assert!(r.ts > last);
        last = r.ts;
    }
    assert_eq!(last, thread.last_reply);
    assert_eq!(w.reply(c, 0, LONG_THREAD_REPLIES), None);
}

#[test]
fn the_hostile_channel_carries_every_hostile_string() {
    let w = world();
    let c = conv(&w, names::HOSTILE);
    let texts: Vec<String> =
        (0..w.message_count(&c.id)).map(|i| w.message(&c.id, i).unwrap().text.unsanitized().to_string()).collect();
    let corpus: Vec<String> = hostile_strings().into_iter().map(|h| h.text).collect();
    assert_eq!(texts, corpus);
    assert!(c.unread > 0, "it shows up as unread, so it gets opened");
}

#[test]
fn message_text_includes_korean_emoji_sequences_and_threads_with_reactions() {
    let w = world();
    let c = &conv(&w, "general").id;
    let all: Vec<Message> = (0..w.message_count(c)).map(|i| w.message(c, i).unwrap()).collect();
    let big = &conv(&w, names::BIG_HISTORY).id;
    let more: Vec<Message> = (0..2000).map(|i| w.message(big, i).unwrap()).collect();
    let any = |p: &dyn Fn(&Message) -> bool| all.iter().chain(&more).any(p);
    assert!(any(&|m| m.text.unsanitized().chars().any(|ch| ('\u{AC00}'..='\u{D7A3}').contains(&ch))), "Korean");
    assert!(any(&|m| m.text.unsanitized().contains('\u{200D}')), "emoji ZWJ sequences");
    assert!(any(&|m| m.thread.is_some()), "threads");
    assert!(any(&|m| !m.reactions.is_empty()), "reaction pills");
    assert!(any(&|m| m.edited), "edited messages");
}

/// Remote parties name things too: hostile names exist, and each one is drawn as harmless text
/// (no control, bidi or zero-width character) that is not empty.
#[test]
fn hostile_names_exist_and_sanitise_to_printable_text() {
    let w = world();
    let s = w.snapshot();
    let bad = |c: char| {
        c.is_control()
            || ('\u{200B}'..='\u{200F}').contains(&c)
            || ('\u{202A}'..='\u{202E}').contains(&c)
            || ('\u{2066}'..='\u{2069}').contains(&c)
            || c == '\u{FEFF}'
    };
    let names: Vec<&Remote> = s
        .workspaces
        .iter()
        .map(|x| &x.name)
        .chain(s.users.iter().flat_map(|u| [&u.name, &u.display_name]))
        .chain(s.sections.iter().map(|x| &x.name))
        .chain(s.conversations.iter().map(|x| &x.name))
        .collect();
    let hostile = names.iter().filter(|n| n.unsanitized().chars().any(bad)).count();
    assert!(hostile >= 4, "a user's two names, their DM and a section: {hostile}");
    for n in names {
        let safe = n.line();
        assert!(!safe.is_empty() && !safe.as_str().chars().any(bad), "{n:?} -> {safe:?}");
    }
    let c = conv(&w, names::HOSTILE);
    let first = w.message(&c.id, 0).unwrap();
    assert!(first.reactions.iter().any(|r| r.name.unsanitized().contains('\x1b')), "a hostile reaction name");
    let posters: HashSet<_> = (0..w.message_count(&c.id)).map(|i| w.message(&c.id, i).unwrap().user).collect();
    let mallory = s.users.iter().find(|u| u.name.unsanitized().starts_with("mallory")).unwrap();
    assert!(posters.contains(&mallory.id), "the hostile user posts there");
    assert!(s.conversations.iter().any(|c| c.kind == ConversationKind::Dm { user: mallory.id.clone() }));
}

/// Every hostile string, sanitised, keeps no escape, control (but line breaks), bidi or
/// zero-width character.
#[test]
fn every_hostile_string_sanitises_to_harmless_text() {
    for h in hostile_strings() {
        let block = Remote::new(h.text.clone()).block();
        let line = Remote::new(h.text).line();
        for (out, breaks) in [(block.as_str(), true), (line.as_str(), false)] {
            assert!(
                !out.chars().any(|c| (c.is_control() && !(breaks && c == '\n'))
                    || ('\u{202A}'..='\u{202E}').contains(&c)
                    || ('\u{2066}'..='\u{2069}').contains(&c)
                    || c == '\u{200B}'),
                "{}: {out:?}",
                h.what
            );
        }
    }
}

#[test]
fn history_pages_walk_back_to_the_first_message() {
    let w = world();
    let big = &conv(&w, names::BIG_HISTORY).id;
    let (newest, done) = w.history(big, None, 200);
    assert_eq!((newest.len(), done), (200, false));
    assert_eq!(newest.last(), w.message(big, BIG_HISTORY_MESSAGES - 1).as_ref());
    let mut before = Some(newest[0].ts);
    let mut seen = newest.len();
    while let Some(b) = before {
        let (page, done) = w.history(big, Some(b), 200);
        assert!(page.iter().all(|m| m.ts < b), "older than asked");
        assert!(page.windows(2).all(|p| p[0].ts < p[1].ts), "oldest first");
        seen += page.len();
        before = if done { None } else { Some(page[0].ts) };
    }
    assert_eq!(seen, BIG_HISTORY_MESSAGES, "every message once");
    let m = w.message(big, 1234).unwrap();
    assert_eq!(w.index_of(big, m.ts), Some(1234));
    assert_eq!(w.index_of(big, Ts(m.ts.0 + 1)), None);
}

#[test]
fn a_thread_starts_with_its_message_and_pages_through_its_replies() {
    let w = world();
    let c = &conv(&w, names::LONG_THREADS).id;
    let root = w.message(c, 0).unwrap();
    let (last, done) = w.thread(c, root.ts, None, 100).unwrap();
    assert_eq!((last.len(), done), (100, false));
    assert_eq!(last.last(), w.reply(c, 0, LONG_THREAD_REPLIES - 1).as_ref());
    let mut all = last;
    while !done_paging(&w, c, root.ts, &mut all) {}
    assert_eq!(all.len(), LONG_THREAD_REPLIES as usize + 1);
    assert_eq!(all[0], root, "the thread's own message first");
    assert!(all.windows(2).all(|p| p[0].ts < p[1].ts));
    // A message without replies is a thread of one.
    let plain = (0..w.message_count(c)).find_map(|i| w.message(c, i).filter(|m| m.thread.is_none())).unwrap();
    assert_eq!(w.thread(c, plain.ts, None, 100), Some((vec![plain.clone()], true)));
    assert_eq!(w.thread(c, Ts(1), None, 100), None, "no such message");
}

/// Prepend the page before `all[0]`; `true` once the first message is in.
fn done_paging(w: &World, c: &ConversationId, thread: Ts, all: &mut Vec<Message>) -> bool {
    let (mut page, done) = w.thread(c, thread, Some(all[0].ts), 100).unwrap();
    page.append(all);
    *all = page;
    done
}
