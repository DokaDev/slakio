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

/// The conversation's pane: the one that is no pane's thread panel.
fn main_id(w: &Work) -> Option<PaneId> {
    w.ids().into_iter().find(|id| w.owner(*id).is_none())
}

fn main_pane(w: &Work) -> Option<&Pane> {
    main_id(w).and_then(|id| w.pane(id))
}

/// The thread panel: the pane the conversation's pane opened.
fn thread_id(w: &Work) -> Option<PaneId> {
    main_pane(w).and_then(|p| p.thread)
}

fn thread_pane(w: &Work) -> Option<&Pane> {
    thread_id(w).and_then(|id| w.pane(id))
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
    let tl = w.timeline(main_pane(&w).unwrap());
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
    assert_eq!(w.active(), thread_id(&w));
    let thread = thread_pane(&w).unwrap();
    assert!(thread.is_thread() && w.timeline(thread).items.len() >= 200);
    let first = thread.target.clone();
    let main = w.beside(-1).unwrap();
    assert_eq!(Some(main), main_id(&w));
    w.activate(main);
    w.with_pane(|p, tl| p.select_index(1, tl));
    let second = w.open_thread();
    assert_eq!(second, thread_id(&w));
    assert_ne!(thread_pane(&w).unwrap().target, first, "replaced");
    assert_eq!(w.close(), (None, Some(main)), "the conversation takes the keyboard");
    w.activate(main);
    assert!(thread_id(&w).is_none() && main_pane(&w).is_some());
    w.close();
    assert!(main_pane(&w).is_none() && w.ids().is_empty(), "closing the conversation empties the work area");
}

#[test]
fn back_and_forward_walk_the_main_panes_history() {
    let m = model();
    let mut w = Work::default();
    assert!(w.back().is_none() && w.forward().is_none());
    for name in ["backend", "incidents", "general"] {
        w.open(conversation(&m, name));
    }
    let at = |w: &Work| main_pane(w).unwrap().target.clone();
    assert!(w.back().is_some());
    assert_eq!(at(&w), conversation(&m, "incidents"));
    assert!(w.back().is_some());
    assert_eq!(at(&w), conversation(&m, "backend"));
    assert!(w.back().is_none());
    assert!(w.forward().is_some());
    assert_eq!(at(&w), conversation(&m, "incidents"));
    w.open(conversation(&m, "random"));
    assert!(w.forward().is_none(), "opening something new clears forward");
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
    let main = main_pane(&w).unwrap();
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
    assert!(main_pane(&w).is_none());
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
    let n = w.timeline(main_pane(&w).unwrap()).items.len();
    assert_eq!(w.timeline(thread_pane(&w).unwrap()).items.len(), n);
    // Both panes select messages near the top; one older page comes, both keep their messages.
    w.with_pane(|p, tl| p.select_index(3, tl));
    let (ts3, ts5) = {
        let tl = w.timeline(main_pane(&w).unwrap());
        (tl.items[3].ts, tl.items[5].ts)
    };
    w.activate(main_id(&w).unwrap());
    w.with_pane(|p, _| p.selected = Some(ts5));
    let asked = w.take_requests();
    assert_eq!(asked.len(), 1, "one request for the target");
    answer(&mut w, &m, &mut b, asked);
    let tl = w.timeline(main_pane(&w).unwrap());
    let (main, thread) = (main_pane(&w).unwrap(), thread_pane(&w).unwrap());
    assert!(tl.items.len() > n, "older messages arrived");
    assert_eq!(tl.items[thread.selected_index(tl).unwrap()].ts, ts3);
    assert_eq!(tl.items[main.selected_index(tl).unwrap()].ts, ts5);
    // One draft for the target: written in one pane, it is there in the other.
    w.with_draft(|c| c.insert("hi"));
    assert_eq!(w.draft(main_pane(&w).unwrap()).text(), "hi");
    // Closing one pane keeps the messages for the other; closing both drops them.
    w.activate(thread_id(&w).unwrap());
    w.close();
    assert!(w.timelines.get(&target).is_some());
    w.activate(main_id(&w).unwrap());
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
    assert_eq!(w.draft(main_pane(&w).unwrap()).text(), "half written");
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
    assert_eq!(w.draft(main_pane(&w).unwrap()).text(), "keep me", "nothing sent, nothing lost");
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
    assert!(main_pane(&w).is_none(), "the conversation is closed");
    assert_eq!(w.drafts.get(&gone).map(Composer::text), Some("unsent"), "its draft is not dropped silently");
    assert!(w.unsent());
}

#[test]
fn a_panes_role_comes_from_its_relation_not_its_place_in_the_tree() {
    let m = model();
    let mut b = crate::demo::DemoBackend::new(World::demo());
    let mut w = Work::default();
    w.open(conversation(&m, "long-threads"));
    pump(&mut w, &m, &mut b);
    w.with_pane(|p, tl| p.select_index(0, tl));
    let panel = w.open_thread().unwrap();
    let owner = w.owner(panel).unwrap();
    // The thread panel placed first: it is still the panel, and the conversation its owner.
    w.tabs.current_tab_mut().unwrap().root = Node::split(Dir::Row, THREAD_SHARE, Node::Leaf(panel), Node::Leaf(owner));
    assert_eq!(w.ids(), vec![panel, owner]);
    assert_eq!((w.owner(panel), w.owner(owner)), (Some(owner), None));
    assert_eq!(w.home_id(), Some(owner), "the list opens into the conversation, not the panel");
    assert!(w.open_thread().is_none(), "a thread opens no thread");
    assert_eq!(w.close(), (None, Some(owner)), "closing the panel hands the keyboard to its owner");
    assert!(w.pane(owner).unwrap().thread.is_none(), "the relation is gone with the panel");
    assert!(!w.has_panel());
    w.activate(owner);
    let (closed, next) = w.close();
    assert_eq!((closed, next), (Some(conversation(&m, "long-threads")), None));
}

#[test]
fn closing_a_pane_closes_the_thread_panel_it_opened() {
    let m = model();
    let mut b = crate::demo::DemoBackend::new(World::demo());
    let mut w = Work::default();
    w.open(conversation(&m, "long-threads"));
    pump(&mut w, &m, &mut b);
    w.with_pane(|p, tl| p.select_index(0, tl));
    let panel = w.open_thread().unwrap();
    let owner = w.owner(panel).unwrap();
    w.activate(owner);
    let (closed, next) = w.close();
    assert!(closed.is_some() && next.is_none());
    assert!(w.pane(panel).is_none() && w.ids().is_empty(), "the panel went with its owner");
}

#[test]
fn the_history_is_the_panes_and_stays_with_it_when_it_shows_something_else() {
    let m = model();
    let mut w = Work::default();
    let [backend, incidents, general] = ["backend", "incidents", "general"].map(|n| conversation(&m, n));
    let id = w.open(backend.clone()).unwrap();
    assert_eq!(w.open(incidents.clone()), Some(id), "the same pane shows the next conversation");
    w.open(general.clone());
    let p = w.pane(id).unwrap();
    assert_eq!(p.history.back, vec![backend.clone(), incidents.clone()]);
    assert!(p.history.forward.is_empty());
    assert_eq!(w.back(), Some(id));
    assert_eq!(w.pane(id).unwrap().history.forward, vec![general.clone()]);
    // Closed, the work area keeps the pane's history: Back opens it again where it was.
    w.close();
    assert!(w.ids().is_empty());
    assert!(w.forward().is_none(), "nothing forward of an empty work area");
    let again = w.back().expect("the pane closed last");
    let p = w.pane(again).unwrap();
    assert_eq!(p.target, incidents);
    assert_eq!((p.history.back.clone(), p.history.forward.clone()), (vec![backend], vec![general]));
    assert!(w.back().is_some() && w.back().is_none(), "one step back, then nothing");
}

#[test]
fn a_history_keeps_its_last_fifty_and_drops_what_is_gone() {
    let mut h = crate::app::pane::History::default();
    let m = model();
    let names = ["backend", "incidents"];
    for i in 0..60 {
        h.left(conversation(&m, names[i % 2]));
    }
    assert_eq!(h.back.len(), crate::app::pane::HISTORY);
    let gone =
        Target::Conversation { workspace: WorkspaceId::new("TDEMOA"), conversation: ConversationId::new("CNOPE") };
    h.forward.push(gone.clone());
    h.retain(|t| *t != gone);
    assert!(h.forward.is_empty());
}
