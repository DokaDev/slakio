//! The demo backend (`slakio --demo`): answers every command from the invented world of
//! `slakio-world`, at once and without any network. It is the only backend for now; the
//! binary's wiring is the one place that names it.

use slakio_core::backend::{Backend, Capabilities, Command, Envelope, Event, Generation, Page};
use slakio_core::model::Target;
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
        Capabilities { demo: true }
    }

    fn send(&mut self, generation: Generation, command: Command) {
        let event = match command {
            Command::Boot => Event::Booted(Box::new(self.world.snapshot().clone())),
            Command::History { target, before, limit } => {
                let limit = limit as usize;
                let (messages, complete) = match &target {
                    Target::Conversation { conversation, .. } => self.world.history(conversation, before, limit),
                    Target::Thread { conversation, thread, .. } => {
                        self.world.thread(conversation, *thread, before, limit).unwrap_or((Vec::new(), true))
                    }
                };
                Event::History(Box::new(Page { target, messages, complete }))
            }
        };
        self.ready.push_back(Envelope { generation, event });
    }

    fn poll(&mut self) -> Option<Envelope> {
        self.ready.pop_front()
    }
}
