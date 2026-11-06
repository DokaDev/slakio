//! The demo backend (`slakio --demo`): answers every request from the invented world of
//! `slakio-world`, at once and without any network. It is the only backend for now; the
//! binary's wiring is the one place that names it.

use slakio_core::backend::{Backend, Capabilities, Command, Envelope, Event, Page, Request};
use slakio_core::model::{Target, Ts};
use slakio_world::World;
use std::collections::VecDeque;

pub struct DemoBackend {
    world: World,
    ready: VecDeque<Envelope>,
}

impl DemoBackend {
    pub fn new(world: World) -> Self {
        Self { world, ready: VecDeque::new() }
    }
}

impl Backend for DemoBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities { demo: true, read_only: false }
    }

    fn send(&mut self, request: Request) {
        let event = match request.command {
            // One workspace's boot answers its part only.
            Command::Boot => Event::Booted(Box::new(match &request.workspace {
                Some(ws) => self.world.snapshot().only(ws),
                None => self.world.snapshot().clone(),
            })),
            Command::History { target, before, limit } => self.page(target, before, limit),
            Command::Catchup { target, limit } => self.page(target, None, limit),
            Command::Around { target, at, limit } => self.around(target, at, limit),
            // A change that does not apply changes nothing; the answer is the list as it is.
            Command::Save { item, change } => {
                self.world.change_saved(&item, change);
                Event::Saved { items: self.world.saved().to_vec(), taken: self.world.snapshot().taken }
            }
        };
        self.ready.push_back(Envelope { generation: request.id, event });
    }

    fn poll(&mut self) -> Option<Envelope> {
        self.ready.pop_front()
    }
}

impl DemoBackend {
    /// Up to `limit` messages of `target` older than `before` (the newest when `None`).
    fn page(&self, target: Target, before: Option<Ts>, limit: u32) -> Event {
        let limit = limit as usize;
        let (messages, complete) = match &target {
            Target::Conversation { conversation, .. } => self.world.history(conversation, before, limit),
            Target::Thread { conversation, thread, .. } => {
                self.world.thread(conversation, *thread, before, limit).unwrap_or((Vec::new(), true))
            }
        };
        Event::History(Box::new(Page { target, messages, complete, users: Vec::new(), cut: false }))
    }

    /// Up to `limit` messages of `target` up to the one at `at`, and the newest `limit` after it
    /// when they reach back to it (as Slack's are read); else the page says its newest are not
    /// in it. A thread's from its start.
    fn around(&self, target: Target, at: Ts, limit: u32) -> Event {
        let Target::Conversation { conversation, .. } = &target else { return self.page(target, None, limit) };
        let (mut messages, complete) = self.world.history(conversation, Some(Ts(at.0 + 1)), limit as usize);
        let newest = self.world.history(conversation, None, limit as usize).0;
        let reaches = newest.first().is_some_and(|m| m.ts <= at) || newest.last().is_none_or(|m| m.ts <= at);
        if reaches {
            messages.extend(newest.into_iter().filter(|m| m.ts > at));
        }
        Event::History(Box::new(Page { target, messages, complete, users: Vec::new(), cut: !reaches }))
    }
}
