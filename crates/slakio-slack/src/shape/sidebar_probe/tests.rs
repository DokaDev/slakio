use super::*;
use serde_json::json;
use slakio_core::model::{Org, Presence, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;

fn log(tag: &str) -> (Diag, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("slakio-sidebar-probe-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("pictures.log");
    (Diag::to(path.clone()), path)
}

/// The lines of the log, each without its time.
fn lines(d: &Diag, path: &std::path::Path) -> Vec<String> {
    d.flush();
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    text.lines().map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest).to_string()).collect()
}

fn bot(id: &str) -> User {
    User {
        id: UserId::new(id),
        workspace: WorkspaceId::new("T1"),
        name: Remote::new("robot"),
        display_name: Remote::new("Robot"),
        org: Org::Own,
        bot: true,
        presence: Presence::Unknown,
        avatar: None,
        no_photo: false,
        avatar_guessed: false,
    }
}

#[test]
fn starred_lines_tally_kind_open_and_source() {
    let (d, path) = log("starred");
    let boot: wire::UserBoot = serde_json::from_value(json!({
        "starred": ["C1", "C2", "G1", "D1", "D2", "D4", "XGONE"],
        "is_open": ["G1", "D4"],
        "channels": [
            {"id": "C1", "name": "one", "is_starred": true},
            {"id": "C2", "name": "two"},
            {"id": "C3", "name": "three", "is_starred": true},
            {"id": "C4", "name": "four"},
            {"id": "G1", "name": "mpdm-a--b-1", "is_mpim": true},
        ],
        "ims": [
            {"id": "D1", "user": "U1", "is_open": false},
            {"id": "D2", "user": "UB", "is_open": true},
            {"id": "D3", "user": "U0", "is_starred": true},
            {"id": "D4", "user": "U4", "is_open": false},
            {"id": "D5", "user": "U5"},
        ],
    }))
    .unwrap();
    starred(&d, &boot, &[bot("UB")], "U0");
    assert_eq!(
        lines(&d, &path),
        [
            "boot-shape field starred kind=app self=n open=y listed_open=n source=list n=1",
            "boot-shape field starred kind=channel self=n open=- listed_open=n source=both n=1",
            "boot-shape field starred kind=channel self=n open=- listed_open=n source=flag n=1",
            "boot-shape field starred kind=channel self=n open=- listed_open=n source=list n=1",
            "boot-shape field starred kind=im self=n open=n listed_open=n source=list n=1",
            "boot-shape field starred kind=im self=n open=n listed_open=y source=list n=1",
            "boot-shape field starred kind=im self=y open=- listed_open=n source=flag n=1",
            "boot-shape field starred kind=mpim self=n open=- listed_open=y source=list n=1",
            "boot-shape field starred kind=unknown self=n open=- listed_open=n source=list n=1",
            "boot-shape field starred-total count=9",
        ]
    );
}

#[test]
fn channel_share_lines_tally_flags_and_counts() {
    let (d, path) = log("share");
    let channels = json!([
        {"id": "C1", "is_org_shared": true, "is_shared": true},
        {"id": "C2", "is_ext_shared": true, "is_org_shared": false, "is_shared": true,
         "is_pending_ext_shared": false, "connected_team_ids": ["T1", "T2"], "shared_team_ids": ["T1"]},
        {"id": "C3", "is_ext_shared": true, "is_org_shared": false, "is_shared": true,
         "is_pending_ext_shared": false, "connected_team_ids": ["T1", "T2"], "shared_team_ids": ["T1"]},
        {"id": "C4", "is_ext_shared": "yes", "connected_team_ids": ["a", "b", "c", "d", "e", "f", "g", "h", "i"]},
        {"id": "C5", "is_ext_shared": true, "is_archived": true},
        {"id": "G1", "is_mpim": true, "name": "mpdm-a--b-1", "is_ext_shared": true},
        {"id": "G2", "is_mpim": true, "name": "Our group"},
        {"id": "G3", "is_mpim": true},
    ]);
    channel_share(&d, channels.as_array().unwrap());
    assert_eq!(
        lines(&d, &path),
        [
            "boot-shape field channel-share ext=- org=- shared=- pending=- connected=9+ shared_teams=- n=1",
            "boot-shape field channel-share ext=- org=y shared=y pending=- connected=- shared_teams=- n=1",
            "boot-shape field channel-share ext=y org=n shared=y pending=n connected=2 shared_teams=1 n=2",
            "boot-shape field mpim-name form=absent n=1",
            "boot-shape field mpim-name form=mpdm n=1",
            "boot-shape field mpim-name form=other n=1",
        ]
    );
}
