//! What the unit tests share: a scratch directory of their own, a cache opened in it, and pages
//! of the fake world.

use crate::db::Store;
use slakio_core::backend::Page;
use slakio_core::model::{ConversationId, Target, WorkspaceId};
use slakio_world::World;
use std::path::PathBuf;

/// An empty scratch directory for test `tag`, under the process's temp directory.
pub fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-store-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The cache in a new scratch directory, as its writer.
pub fn writer(tag: &str) -> Store {
    let opened = Store::open(&scratch(tag).join("cache")).unwrap();
    assert!(!opened.rebuilt && !opened.store.read_only());
    opened.store
}

/// The conversation `name` of `world`'s first workspace, as a target.
pub fn target(world: &World, name: &str) -> Target {
    let ws = &world.snapshot().workspaces[0].id;
    let c = world.find(ws.as_str(), name).unwrap_or_else(|| panic!("{name} in the world"));
    Target::Conversation { workspace: ws.clone(), conversation: c.id.clone() }
}

/// A conversation target that is not in the world.
pub fn other(ws: &str, conv: &str) -> Target {
    Target::Conversation { workspace: WorkspaceId::new(ws), conversation: ConversationId::new(conv) }
}

/// The world's page of `target` older than `before`.
pub fn page(world: &World, target: &Target, before: Option<slakio_core::model::Ts>, limit: usize) -> Page {
    let (messages, complete) = world.history(target.conversation(), before, limit);
    Page { target: target.clone(), messages, complete, users: Vec::new() }
}
