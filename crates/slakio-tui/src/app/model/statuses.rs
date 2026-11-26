//! People's statuses across a whole new boot: a status is kept for the session only (never
//! stored), so a boot whose record of someone says nothing of one (a cached boot, a person the
//! start did not look up) keeps the status known of them.

use slakio_core::model::{User, UserId, WorkspaceId};
use std::collections::HashMap;

/// `new` (the people of a new boot), each keeping the status known of them in `old` unless its
/// record says what it is now ([`User::status_over`]).
pub(super) fn keep(old: &[User], new: &mut [User]) {
    let known: HashMap<(&WorkspaceId, &UserId), &slakio_core::model::Status> =
        old.iter().filter_map(|u| Some(((&u.workspace, &u.id), u.status.as_ref()?))).collect();
    for u in new {
        u.status = u.status_over(known.get(&(&u.workspace, &u.id)).copied());
    }
}
