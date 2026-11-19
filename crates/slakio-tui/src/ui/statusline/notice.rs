//! The notice in the status line, fitted to the room it has: one about a workspace keeps its
//! name, a host that opens its end, and one about a file its name's extension; a download's
//! line drops the name before how much has come and the key that stops it.

use crate::app::App;
use crate::app::status::NOTICE_MAX;
use crate::opener::host_tail;
use crate::text::{clip, clip_name, width};
use slakio_core::i18n::{Label, Msg};

/// The most of a workspace's name the hints make room for in a notice about it.
const NOTICE_NAME: usize = 24;

/// The least of a workspace's name a notice about it keeps, however short the line.
const NOTICE_NAME_KEPT: usize = 8;

/// The name of the workspace notice `msg` is about.
pub(super) fn named(msg: &Msg) -> Option<&str> {
    match msg {
        Msg::FailWorkspace { workspace, .. } | Msg::FailWorkspaceAuth { workspace } => Some(workspace),
        _ => None,
    }
}

/// Notice `msg` about a workspace, with `name` for the workspace's.
fn renamed(msg: &Msg, name: String) -> Msg {
    match msg {
        Msg::FailWorkspace { why, .. } => Msg::FailWorkspace { workspace: name, why: why.clone() },
        Msg::FailWorkspaceAuth { .. } => Msg::FailWorkspaceAuth { workspace: name },
        other => other.clone(),
    }
}

/// The room the hints make for notice `msg`: [`NOTICE_MAX`], or more for one about a workspace,
/// so its whole reason fits beside its name cut to [`NOTICE_NAME`] cells.
pub(super) fn notice_max(app: &App, msg: &Msg) -> usize {
    if let Msg::OpenOpening { host } = msg {
        return width(&app.i18n.msg(&Msg::OpenOpening { host: host_tail(host, app.host_room()) }));
    }
    // A file saved: its name and its folder, as far as a workspace's notice goes.
    if let Some((_, Some(_))) = filed(msg) {
        return NOTICE_MAX.max(width(&app.i18n.msg(msg)).min(NOTICE_MAX + NOTICE_NAME));
    }
    let Some(name) = named(msg) else { return NOTICE_MAX };
    NOTICE_MAX.max(width(&app.i18n.msg(&renamed(msg, clip(name, NOTICE_NAME)))))
}

/// Notice `msg` in `room` cells. One about a workspace always names it: its name is cut with
/// `…` (never under [`NOTICE_NAME_KEPT`] cells) before the words after it lose their end.
pub(super) fn fit_notice(app: &App, msg: &Msg, room: usize) -> String {
    let whole = app.i18n.msg(msg).to_string();
    match msg {
        _ if width(&whole) <= room => return whole,
        Msg::JumpDateQuiet { .. } => return clip(&app.i18n.label(Label::JumpDateQuietShort).to_string(), room),
        _ => {}
    }
    // A host that opens is cut from its start, to the cells free.
    if let Msg::OpenOpening { host } = msg {
        let around = width(&whole).saturating_sub(width(host));
        return clip(&app.i18n.msg(&Msg::OpenOpening { host: host_tail(host, room.saturating_sub(around)) }), room);
    }
    if let Some(fit) = fit_file(app, msg, room) {
        return fit;
    }
    let Some(name) = named(msg) else { return clip(&whole, room) };
    let around = width(&whole).saturating_sub(width(name));
    let kept = room.saturating_sub(around).max(NOTICE_NAME_KEPT.min(width(name)));
    clip(&app.i18n.msg(&renamed(msg, clip(name, kept))), room)
}

/// The least of a file's name a notice about it keeps before its folder is cut (or a download's
/// line drops the name).
const FILE_NAME_KEPT: usize = 10;

/// The least of a folder's end a notice about a file saved keeps.
const FOLDER_KEPT: usize = 8;

/// The name of the file notice `msg` is about, and the folder it was saved in when it says so.
fn filed(msg: &Msg) -> Option<(&str, Option<&str>)> {
    match msg {
        Msg::FileWaiting { name, .. }
        | Msg::FileDownloading { name, .. }
        | Msg::FileProgress { name, .. }
        | Msg::FileAlready { name }
        | Msg::FileCancelled { name } => Some((name, None)),
        Msg::FileSaved { name, folder }
        | Msg::FileSavedOnly { name, folder }
        | Msg::FileSavedUnmarked { name, folder }
        | Msg::FileSavedDemo { name, folder } => Some((name, Some(folder))),
        _ => None,
    }
}

/// Notice `msg` about a file, with `name` (and `folder`) for its own.
fn refiled(msg: &Msg, name: String, folder: String) -> Msg {
    match msg.clone() {
        Msg::FileWaiting { stop, .. } => Msg::FileWaiting { name, stop },
        Msg::FileDownloading { stop, .. } => Msg::FileDownloading { name, stop },
        Msg::FileProgress { percent, stop, .. } => Msg::FileProgress { name, percent, stop },
        Msg::FileAlready { .. } => Msg::FileAlready { name },
        Msg::FileCancelled { .. } => Msg::FileCancelled { name },
        Msg::FileSaved { .. } => Msg::FileSaved { name, folder },
        Msg::FileSavedOnly { .. } => Msg::FileSavedOnly { name, folder },
        Msg::FileSavedUnmarked { .. } => Msg::FileSavedUnmarked { name, folder },
        Msg::FileSavedDemo { .. } => Msg::FileSavedDemo { name, folder },
        other => other,
    }
}

/// A download's line without the name: how much has come, and the key that stops it.
fn nameless(msg: &Msg) -> Option<Msg> {
    Some(match msg.clone() {
        Msg::FileProgress { percent, stop, .. } => Msg::FileProgressShort { percent, stop },
        Msg::FileWaiting { stop, .. } | Msg::FileDownloading { stop, .. } => Msg::FileStopShort { stop },
        Msg::FileDownloadingMany { count, stop } => Msg::FileManyShort { count, stop },
        _ => return None,
    })
}

/// Notice `msg` about a file in `room` cells, `None` when it is not about one: a file saved has
/// its folder cut from its start first (down to [`FOLDER_KEPT`] cells); then the name is cut in
/// its middle (its extension kept) down to [`FILE_NAME_KEPT`] cells; then a download's line
/// drops it (how much has come and the key that stops it stay).
fn fit_file(app: &App, msg: &Msg, room: usize) -> Option<String> {
    let said = |m: &Msg| app.i18n.msg(m).to_string();
    let Some((name, folder)) = filed(msg) else {
        return nameless(msg).map(|m| clip(&said(&m), room));
    };
    let folder = folder.unwrap_or_default();
    let around = width(&said(msg)).saturating_sub(width(folder));
    let folder = host_tail(folder, room.saturating_sub(around).max(FOLDER_KEPT.min(width(folder))));
    let msg = refiled(msg, name.to_string(), folder.clone());
    let around = width(&said(&msg)).saturating_sub(width(name));
    let kept = FILE_NAME_KEPT.min(width(name));
    if room >= around + kept {
        return Some(said(&refiled(&msg, clip_name(name, room - around), folder)));
    }
    if let Some(short) = nameless(&msg) {
        return Some(clip(&said(&short), room));
    }
    Some(clip(&said(&refiled(&msg, clip_name(name, kept), folder)), room))
}
