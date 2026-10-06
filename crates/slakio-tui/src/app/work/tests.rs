use super::*;
use crate::app::model::Model;
use slakio_core::backend::Event;
use slakio_core::model::{ConversationId, WorkspaceId};
use slakio_world::World;

fn model() -> Model {
    Model::new(World::demo().snapshot().clone())
}

fn conversation(m: &Model, name: &str) -> Target {
    let c = (0..).map(|i| m.conversation(i)).find(|c| c.workspace.as_str() == "TDEMOA" && c.name == name).unwrap();
    Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
}

/// Answer the requests `asked` from the demo world.
fn answer(w: &mut Work, m: &Model, backend: &mut crate::demo::DemoBackend, asked: Vec<(Generation, Command)>) {
    use slakio_core::backend::Backend;
    for (g, c) in asked {
        backend.send(g, c);
    }
    while let Some(e) = backend.poll() {
        if let Event::History(page) = e.event {
            assert!(w.on_page(e.generation, &page, m));
        }
    }
}

/// Answer every request from the demo world, as the binary's loop does.
fn pump(w: &mut Work, m: &Model, backend: &mut crate::demo::DemoBackend) {
    loop {
        let asked = w.take_requests();
        if asked.is_empty() {
            return;
        }
        answer(w, m, backend, asked);
    }
}

#[test]
fn opening_loads_the_newest_page_and_reopening_focuses_instead() {
    let m = model();
    let mut b = crate::demo::DemoBackend::new(World::demo());
    let mut w = Work::default();
    let backend = conversation(&m, "backend");
    w.open(backend.clone());
    let asked = w.take_requests();
    assert_eq!(asked.len(), 1);
    w.open(backend);
    assert!(w.take_requests().is_empty(), "already open: no second load");
    answer(&mut w, &m, &mut b, asked);
    pump(&mut w, &m, &mut b);
    let tl = w.timeline(w.main.as_ref().unwrap());
    assert!(!tl.items.is_empty() && tl.complete, "a short channel loads in one page");
}

#[test]
fn a_stale_or_foreign_page_is_dropped() {
    let m = model();
    let mut w = Work::default();
    w.open(conversation(&m, "backend"));
    let (g, _) = w.take_requests().remove(0);
    let other = Page { target: conversation(&m, "incidents"), messages: Vec::new(), complete: true };
    assert!(!w.on_page(g, &other, &m), "another target");
    let same = Page { target: conversation(&m, "backend"), messages: Vec::new(), complete: true };
    assert!(!w.on_page(Generation(g.0 + 7), &same, &m), "another generation");
    assert!(w.on_page(g, &same, &m));
    assert!(!w.on_page(g, &same, &m), "answered already");
}

#[test]
fn enter_opens_the_thread_panel_and_another_thread_replaces_it() {
    let m = model();
    let mut b = crate::demo::DemoBackend::new(World::demo());
    let mut w = Work::default();
    w.open(conversation(&m, "long-threads"));
    pump(&mut w, &m, &mut b);
    w.with_pane(|p, tl| p.select_index(0, tl));
    w.open_thread();
    pump(&mut w, &m, &mut b);
    assert_eq!(w.side, Side::Thread);
    let thread = w.thread.as_ref().unwrap();
    assert!(thread.is_thread() && w.timeline(thread).items.len() >= 200);
    let first = thread.target.clone();
    assert!(w.focus_side(-1));
    w.with_pane(|p, tl| p.select_index(1, tl));
    w.open_thread();
    assert_ne!(w.thread.as_ref().unwrap().target, first, "replaced");
    w.close();
    assert_eq!((w.side, w.thread.is_none(), w.main.is_some()), (Side::Main, true, true));
    w.close();
    assert!(w.main.is_none(), "closing the conversation empties the work area");
}

#[test]
fn back_and_forward_walk_the_main_panes_history() {
    let m = model();
    let mut w = Work::default();
    assert!(!w.back() && !w.forward());
    for name in ["backend", "incidents", "general"] {
        w.open(conversation(&m, name));
    }
    let at = |w: &Work| w.main.as_ref().unwrap().target.clone();
    assert!(w.back());
    assert_eq!(at(&w), conversation(&m, "incidents"));
    assert!(w.back());
    assert_eq!(at(&w), conversation(&m, "backend"));
    assert!(!w.back());
    assert!(w.forward());
    assert_eq!(at(&w), conversation(&m, "incidents"));
    w.open(conversation(&m, "random"));
    assert!(!w.forward(), "opening something new clears forward");
}

#[test]
fn send_echoes_the_composer_as_the_users_message_and_empties_it() {
    let m = model();
    let mut b = crate::demo::DemoBackend::new(World::demo());
    let mut w = Work::default();
    w.open(conversation(&m, "backend"));
    pump(&mut w, &m, &mut b);
    assert!(!w.send(&m), "nothing to send");
    w.with_draft(|c| c.insert("  hello\x1b[2J  "));
    assert!(w.send(&m));
    let main = w.main.as_ref().unwrap();
    let last = w.timeline(main).items.last().unwrap();
    assert!(last.own && last.text.as_str() == "  hello  ");
    assert_eq!(last.author.as_str(), "Me");
    assert!(w.draft(main).is_empty());
}

#[test]
fn clamping_drops_a_conversation_that_is_gone() {
    let m = model();
    let mut w = Work::default();
    w.open(Target::Conversation { workspace: WorkspaceId::new("TDEMOA"), conversation: ConversationId::new("CNOPE") });
    w.clamp(&m);
    assert!(w.main.is_none());
}

#[test]
fn two_panes_on_one_target_share_its_messages_and_load_them_once() {
    let m = model();
    let mut b = crate::demo::DemoBackend::new(World::demo());
    let mut w = Work::default();
    let target = conversation(&m, "big-history");
    w.open(target.clone());
    pump(&mut w, &m, &mut b);
    w.show_beside(target.clone());
    assert!(w.take_requests().is_empty(), "the second pane loads nothing");
    let n = w.timeline(w.main.as_ref().unwrap()).items.len();
    assert_eq!(w.timeline(w.thread.as_ref().unwrap()).items.len(), n);
    // Both panes select messages near the top; one older page comes, both keep their messages.
    w.with_pane(|p, tl| p.select_index(3, tl));
    let (ts3, ts5) = {
        let tl = w.timeline(w.main.as_ref().unwrap());
        (tl.items[3].ts, tl.items[5].ts)
    };
    w.main.as_mut().unwrap().selected = Some(ts5);
    let asked = w.take_requests();
    assert_eq!(asked.len(), 1, "one request for the target");
    answer(&mut w, &m, &mut b, asked);
    let tl = w.timeline(w.main.as_ref().unwrap());
    let (main, thread) = (w.main.as_ref().unwrap(), w.thread.as_ref().unwrap());
    assert!(tl.items.len() > n, "older messages arrived");
    assert_eq!(tl.items[thread.selected_index(tl).unwrap()].ts, ts3);
    assert_eq!(tl.items[main.selected_index(tl).unwrap()].ts, ts5);
    // One draft for the target: written in one pane, it is there in the other.
    w.with_draft(|c| c.insert("hi"));
    assert_eq!(w.draft(w.main.as_ref().unwrap()).text(), "hi");
    // Closing one pane keeps the messages for the other; closing both drops them.
    w.close();
    assert!(w.timelines.get(&target).is_some());
    w.close();
    assert!(w.timelines.get(&target).is_none());
}

#[test]
fn a_page_for_a_closed_pane_is_dropped_and_reopening_loads_afresh() {
    let m = model();
    let mut w = Work::default();
    let backend = conversation(&m, "backend");
    w.open(backend.clone());
    let (g, _) = w.take_requests().remove(0);
    w.close();
    let page = Page { target: backend.clone(), messages: Vec::new(), complete: true };
    assert!(!w.on_page(g, &page, &m), "nobody waits for it");
    w.open(backend);
    let asked = w.take_requests();
    assert_eq!(asked.len(), 1, "loaded again");
    assert!(asked[0].0 > g, "one allocator: every request a new id");
}

#[test]
fn a_draft_outlives_its_pane_and_is_there_when_its_target_opens_again() {
    let m = model();
    let mut w = Work::default();
    let (backend, incidents) = (conversation(&m, "backend"), conversation(&m, "incidents"));
    w.open(backend.clone());
    w.with_draft(|c| c.insert("half written"));
    w.open(incidents.clone());
    assert!(w.unsent(), "a draft of a closed pane is still not sent");
    w.with_draft(|c| c.insert(" "));
    w.with_draft(|c| {
        c.backspace();
    });
    w.close();
    assert!(w.drafts.get(&incidents).is_none(), "an empty draft is not kept");
    w.open(backend.clone());
    assert_eq!(w.draft(w.main.as_ref().unwrap()).text(), "half written");
    w.clamp(&m);
    assert!(w.drafts.get(&backend).is_some(), "kept while its conversation exists");
}

#[test]
fn a_draft_that_cannot_be_sent_stays_in_the_composer() {
    let m = model();
    let mut w = Work::default();
    // A workspace the model does not know: no user to send as.
    w.open(Target::Conversation { workspace: WorkspaceId::new("TNOPE"), conversation: ConversationId::new("C") });
    w.with_draft(|c| c.insert("keep me"));
    assert!(!w.send(&m));
    assert_eq!(w.draft(w.main.as_ref().unwrap()).text(), "keep me", "nothing sent, nothing lost");
}

#[test]
fn a_draft_of_a_conversation_that_is_gone_is_kept_until_quitting_asks() {
    let m = model();
    let mut w = Work::default();
    let gone =
        Target::Conversation { workspace: WorkspaceId::new("TDEMOA"), conversation: ConversationId::new("CNOPE") };
    w.open(gone.clone());
    w.with_draft(|c| c.insert("unsent"));
    w.clamp(&m);
    assert!(w.main.is_none(), "the conversation is closed");
    assert_eq!(w.drafts.get(&gone).map(Composer::text), Some("unsent"), "its draft is not dropped silently");
    assert!(w.unsent());
}
