//! `B side`'s Starred and Apps: a starred DM that is open (listed in Starred), a starred DM that
//! is closed in Slack's sidebar (so Starred does not list it, as in Slack's desktop app) and a DM
//! with an app (in Apps). Starred comes first and Apps last, as Slack orders them; nothing is
//! unread in them. Added once every workspace is built, so no other conversation's id (and so
//! none of its messages) moves.

use crate::World;
use slakio_core::model::{ConversationId, ConversationKind, Section, SectionId, SectionKind};

/// The workspace that has them.
const TEAM: &str = "TDEMOB";

pub(crate) fn add(w: &mut World) {
    let Some(ws) = w.snapshot.workspaces.iter().find(|x| x.id.as_str() == TEAM).cloned() else { return };
    let Some(first) = w.snapshot.sections.iter().position(|s| s.workspace == ws.id) else { return };
    let section = |id: &str, name: &str, kind| Section {
        id: SectionId::new(id),
        workspace: ws.id.clone(),
        name: name.into(),
        kind,
        sort: None,
    };
    let starred = section("SDEMOBS", "Starred", SectionKind::Favorites);
    let apps = section("SDEMOBP", "Apps", SectionKind::Apps);
    w.snapshot.sections.insert(first, starred.clone());
    w.snapshot.sections.push(apps.clone());
    let (users, ws_id, me) = (&w.snapshot.users, &ws.id, &ws.me);
    let of_ws = |bot: bool| users.iter().filter(move |u| &u.workspace == ws_id && &u.id != me && u.bot == bot);
    // The two people after those with a DM in Direct messages, and the first app.
    let (Some(open), Some(shut)) = (of_ws(false).nth(6).cloned(), of_ws(false).nth(5).cloned()) else { return };
    let Some(app) = of_ws(true).next().cloned() else { return };
    let added = [
        (&open, &starred, false, "DDEMOBS001"),
        (&shut, &starred, true, "DDEMOBS002"),
        (&app, &apps, false, "DDEMOBP001"),
    ];
    for (peer, at, closed, id) in added {
        let kind = ConversationKind::Dm { user: peer.id.clone() };
        let posters = vec![ws.me.clone(), peer.id.clone()];
        w.add_conversation(&ws.id, ConversationId::new(id), kind, peer.display_name.clone(), &at.id, posters);
        if let Some(c) = w.snapshot.conversations.last_mut() {
            (c.unread, c.mentions, c.muted, c.closed) = (0, 0, false, closed);
        }
    }
}
