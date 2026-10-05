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

/// Answer every request from the demo world, as the binary's loop does.
fn pump(w: &mut Work, m: &Model, backend: &mut crate::demo::DemoBackend) {
    use slakio_core::backend::Backend;
    loop {
        let asked = w.take_requests();
        if asked.is_empty() {
            return;
        }
        for (g, c) in asked {
            backend.send(g, c);
        }
        while let Some(e) = backend.poll() {
            if let Event::History(page) = e.event {
                assert!(w.on_page(e.generation, &page, m));
            }
        }
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
    w.requests = asked;
    pump(&mut w, &m, &mut b);
    let main = w.main.as_ref().unwrap();
    assert!(!main.items.is_empty() && main.complete, "a short channel loads in one page");
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
    w.with_pane(|p| p.selected = Some(0));
    w.open_thread();
    pump(&mut w, &m, &mut b);
    assert_eq!(w.side, Side::Thread);
    let thread = w.thread.as_ref().unwrap();
    assert!(thread.is_thread() && thread.items.len() >= 200);
    let first = thread.target.clone();
    assert!(w.focus_side(-1));
    w.with_pane(|p| p.selected = Some(1));
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
    w.with_pane(|p| p.composer.insert("  hello\x1b[2J  "));
    assert!(w.send(&m));
    let main = w.main.as_ref().unwrap();
    let last = main.items.last().unwrap();
    assert!(last.own && last.text.as_str() == "  hello  ");
    assert_eq!(last.author.as_str(), "Me");
    assert!(main.composer.is_empty());
}

#[test]
fn clamping_drops_a_conversation_that_is_gone() {
    let m = model();
    let mut w = Work::default();
    w.open(Target::Conversation { workspace: WorkspaceId::new("TDEMOA"), conversation: ConversationId::new("CNOPE") });
    w.clamp(&m);
    assert!(w.main.is_none());
}
