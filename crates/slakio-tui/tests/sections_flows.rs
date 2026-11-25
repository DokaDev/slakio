//! Slack's sidebar sections arriving after the boot answer: they replace the workspace's own,
//! each conversation moves to its section, and the list's cursor stays on its conversation even
//! when the header above it is gone.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Envelope, Event, Generation, Live};
use slakio_core::model::{ConversationId, Section, SectionId, SectionKind};
use slakio_tui::app::model::Row;

fn push(s: &mut Scripted, live: Live) {
    s.b.push(Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) });
    s.pump();
}

/// The conversation under the list's cursor.
fn under_cursor(s: &Scripted) -> Option<String> {
    match s.d.app.list_rows().get(s.d.app.list_cursor())? {
        Row::Conversation(i) => Some(s.d.app.model.conversation(*i).name.unsanitized().to_string()),
        _ => None,
    }
}

#[test]
fn sections_that_come_later_replace_the_workspace_s_and_the_cursor_stays_on_its_conversation() {
    let mut s = Scripted::booted(true, None);
    s.d.list_cursor_on("incidents");
    let starred = s.d.app.model.conversations().iter().find(|c| c.name == "incidents").unwrap().clone();
    let ws = starred.workspace.clone();
    // A DM below the sections that go: its row moves up.
    let dm = (s.d.app.model.conversations().iter())
        .find(|c| c.workspace == ws && c.is_dm())
        .unwrap()
        .name
        .unsanitized()
        .to_string();
    s.d.list_cursor_on(&dm);
    let row_before = s.d.app.list_cursor();
    // Slack lists no Starred now: Channels and Direct messages only, everything in Channels
    // or Direct messages.
    let channels = Section {
        id: SectionId::new(format!("{}:channels", ws.as_str())),
        workspace: ws.clone(),
        name: "Channels".into(),
        kind: SectionKind::Channels,
    };
    let dms = Section {
        id: SectionId::new(format!("{}:dms", ws.as_str())),
        workspace: ws.clone(),
        name: "Direct messages".into(),
        kind: SectionKind::DirectMessages,
    };
    let places: Vec<(ConversationId, SectionId)> = (s.d.app.model.conversations().iter())
        .filter(|c| c.workspace == ws)
        .map(|c| (c.id.clone(), if c.is_dm() { dms.id.clone() } else { channels.id.clone() }))
        .collect();
    let rows_before = s.d.app.list_rows().len();
    push(&mut s, Live::Sections { workspace: ws.clone(), sections: vec![channels.clone(), dms], places });
    let kinds: Vec<SectionKind> = (s.d.app.list_rows().iter())
        .filter_map(|r| match r {
            Row::Section(x) => Some(s.d.app.model.section(*x).kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, [SectionKind::Channels, SectionKind::DirectMessages], "only Slack's sections");
    let moved = s.d.app.model.conversations().iter().find(|c| c.id == starred.id).unwrap();
    assert_eq!(moved.section, channels.id);
    assert_ne!(s.d.app.list_rows().len(), rows_before);
    assert_ne!(s.d.app.list_cursor(), row_before, "its row moved");
    assert_eq!(under_cursor(&s).as_deref(), Some(dm.as_str()), "the cursor stays on its conversation");
}

#[test]
fn sections_of_a_workspace_not_listed_change_nothing() {
    let mut s = Scripted::connected(true);
    s.pump();
    s.b.fail(0, slakio_core::backend::FailKind::Network);
    s.pump();
    let ws = slakio_core::model::WorkspaceId::new("T0NOTLISTED");
    let section = Section {
        id: SectionId::new("T0NOTLISTED:channels"),
        workspace: ws.clone(),
        name: "Channels".into(),
        kind: SectionKind::Channels,
    };
    push(&mut s, Live::Sections { workspace: ws, sections: vec![section], places: Vec::new() });
    s.keys("j");
    assert!(s.d.app.model.workspaces().is_empty());
    assert!(s.d.app.list_rows().is_empty());
}
