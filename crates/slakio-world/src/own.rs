//! What the demo says of the user's own self and the people's statuses: each workspace's own DM
//! (the user's notes, last of its direct messages) and the status of the user and of the first
//! two people (a standard emoji and one of the workspace's own).

use crate::World;
use slakio_core::model::{ConversationId, ConversationKind, SectionKind, Status};

/// The status of person `i` of a workspace (the user's own: `None`).
pub(crate) fn status_of(i: Option<usize>) -> Option<Status> {
    let (emoji, text) = match i {
        None => ("tada", "Shipping"),
        Some(0) => ("palm_tree", "On holiday"),
        Some(1) => ("party-blob", "Celebrating"),
        Some(_) => return None,
    };
    Some(Status { emoji: emoji.into(), text: text.into(), expires: None })
}

/// Each workspace's own DM, with nothing unread in it. Added once every workspace is built, so
/// no other conversation's id (and so none of its messages) moves.
pub(crate) fn add_dms(w: &mut World) {
    for ws in w.snapshot.workspaces.clone() {
        let dms = w.snapshot.sections.iter().find(|s| s.workspace == ws.id && s.kind == SectionKind::DirectMessages);
        let Some(section) = dms.map(|s| s.id.clone()) else { continue };
        let Some(me) = w.snapshot.users.iter().find(|u| u.workspace == ws.id && u.id == ws.me) else { continue };
        let name = me.display_name.clone();
        let id = ConversationId::new(format!("DDEMO{}0000", &ws.id.as_str()[5..]));
        let kind = ConversationKind::Dm { user: ws.me.clone() };
        w.add_conversation(&ws.id, id, kind, name, &section, vec![ws.me.clone()]);
        if let Some(own) = w.snapshot.conversations.last_mut() {
            (own.unread, own.mentions) = (0, 0);
        }
    }
}
