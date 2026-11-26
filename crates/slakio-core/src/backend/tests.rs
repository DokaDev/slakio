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
        Command::History { target: target.clone(), before: None, limit: 28 },
        Command::Catchup { target: target.clone(), limit: 28 },
        Command::Around { target: target.clone(), at: crate::model::Ts(1), limit: 28 },
        Command::Dated { target, at: crate::model::Ts(1), limit: 28 },
        Command::Search { query: "in:#general deploy".into(), page: 1 },
        Command::Activity { cursor: None },
        Command::Activity { cursor: Some("bmV4dA==".into()) },
        Command::Save { item: SavedId::new("S1"), change: SavedChange::Complete },
        Command::Cancel { request: Generation(2) },
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
        avatar: None,
        no_photo: false,
        avatar_guessed: false,
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

#[test]
fn a_workspace_s_new_sections_take_the_place_of_its_own_among_the_others() {
    use crate::model::{Section, SectionId, SectionKind};
    let section = |ws: &str, id: &str| Section {
        id: SectionId::new(format!("{ws}:{id}")),
        workspace: WorkspaceId::new(ws),
        name: crate::sanitize::Remote::new(""),
        kind: SectionKind::Channels,
        sort: None,
    };
    let mut s = Snapshot {
        sections: vec![section("T1", "a"), section("T2", "b"), section("T2", "c"), section("T3", "d")],
        ..Snapshot::default()
    };
    let t2 = WorkspaceId::new("T2");
    // A section of another workspace among the new ones is not taken.
    s.resection(&t2, vec![section("T2", "x"), section("T9", "y")], &[]);
    let ids: Vec<&str> = s.sections.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, ["T1:a", "T2:x", "T3:d"]);
    // Kept from a snapshot without the workspace's sections: nothing changes.
    s.keep_sections(&t2, &Snapshot::default());
    assert_eq!(s.sections.len(), 3);
}
