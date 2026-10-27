use super::*;
use crate::model::{ConversationId, SavedId, Target, WorkspaceId};

fn request(command: Command) -> Request {
    Request { id: Generation(3), workspace: None, command }
}

fn every_command() -> Vec<Command> {
    let ws = WorkspaceId::new("T1");
    let target = Target::Conversation { workspace: ws, conversation: ConversationId::new("C1") };
    vec![
        Command::Boot,
        Command::History { target, before: None, limit: 28 },
        Command::Save { item: SavedId::new("S1"), change: SavedChange::Complete },
    ]
}

#[test]
fn only_a_change_is_refused_and_every_command_is_classified() {
    for command in every_command() {
        let refused = refuse_change(&request(command.clone()));
        assert_eq!(refused.is_some(), command.changes(), "{command:?}");
        if let Some(e) = refused {
            assert_eq!(e.generation, Generation(3));
            assert!(matches!(e.event, Event::Failed { request: Generation(3), kind: FailKind::ReadOnly, .. }));
        }
    }
    assert!(Command::Save { item: SavedId::new("S1"), change: SavedChange::Clear }.changes());
    assert!(!Command::Boot.changes());
}

#[test]
fn a_full_queue_is_busy_and_a_gone_backend_offline() {
    let caps = Capabilities { demo: false, read_only: false };
    let (handle, end) = BackendHandle::pair(caps, 1);
    assert!(handle.send(request(Command::Boot)).is_ok());
    let full = handle.send(request(Command::Boot)).expect_err("no room");
    assert!(matches!(full.event, Event::Failed { kind: FailKind::Busy, .. }), "{full:?}");
    drop(end);
    let gone = handle.send(request(Command::Boot)).expect_err("gone");
    assert!(matches!(gone.event, Event::Failed { kind: FailKind::Offline, .. }), "{gone:?}");
}
