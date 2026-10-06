//! The requests for the backend: one allocator of request ids (each request's [`Generation`])
//! for the whole app, and the queue the binary's loop takes them from. An answer names the id
//! of its request, so whoever asked can tell a late answer from the one it waits for.

use slakio_core::backend::{Command, Generation};

#[derive(Clone, Debug, Default)]
pub struct Requests {
    last: Generation,
    queue: Vec<(Generation, Command)>,
}

impl Requests {
    /// Queue `command` under a new id, handed back.
    pub fn ask(&mut self, command: Command) -> Generation {
        self.last = self.last.next();
        self.queue.push((self.last, command));
        self.last
    }

    /// The requests queued since the last call, oldest first.
    pub fn take(&mut self) -> Vec<(Generation, Command)> {
        std::mem::take(&mut self.queue)
    }
}
