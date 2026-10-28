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

#[test]
fn a_snapshot_cut_to_one_workspace_keeps_its_part_only() {
    use crate::model::{Org, Presence, User, UserId, Workspace, WorkspaceColor};
    use crate::sanitize::Remote;
    let user = |ws: &str, id: &str| User {
        id: UserId::new(id),
        workspace: WorkspaceId::new(ws),
        name: Remote::new(id),
        display_name: Remote::new(id),
        org: Org::Own,
        bot: false,
        presence: Presence::Unknown,
    };
    let ws = |id: &str| Workspace {
        id: WorkspaceId::new(id),
        name: Remote::new(id),
        color: WorkspaceColor(0),
        me: UserId::new("U1"),
    };
    // The same person listed by both workspaces (a Slack Connect peer).
    let s = Snapshot {
        workspaces: vec![ws("T1"), ws("T2")],
        users: vec![user("T1", "U1"), user("T2", "U1"), user("T2", "U2")],
        saved: None,
        taken: crate::model::Ts(7),
        ..Snapshot::default()
    };
    let only = s.only(&WorkspaceId::new("T2"));
    assert_eq!(only.workspaces, vec![ws("T2")]);
    assert_eq!(only.users, vec![user("T2", "U1"), user("T2", "U2")]);
    assert_eq!((only.saved, only.taken), (None, crate::model::Ts(7)), "unknown stays unknown");
    assert_eq!(s.only(&WorkspaceId::new("T9")).workspaces, vec![]);
}
