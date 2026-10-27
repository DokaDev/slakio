//! A backend whose answers the test scripts: every request is held until the test answers it
//! (from the demo world), fails it, answers it twice, or never; a read-only one refuses a change
//! on the spot. `log` holds every request carried out, so a test can tell that a refused change
//! never went anywhere. Driven like the binary's loop drives a backend (`exchange`), with the
//! demo harness for keys and frames.

use crate::demo::Demo;
use slakio_core::backend::{Backend, Capabilities, Envelope, FailKind, Request, Snapshot, refuse_change};
use slakio_core::i18n::Lang;
use slakio_tui::app::App;
use slakio_tui::demo::DemoBackend;
use slakio_tui::keymap::parse_keys;
use slakio_tui::theme::Theme;
use slakio_world::World;
use std::collections::VecDeque;

pub struct ScriptedBackend {
    pub caps: Capabilities,
    /// Requests received and not answered yet, oldest first.
    pub held: Vec<Request>,
    /// Every request carried out (answered from the world).
    pub log: Vec<Request>,
    /// The boot answer, when the test replaces the world's.
    pub snapshot: Option<Snapshot>,
    world: DemoBackend,
    ready: VecDeque<Envelope>,
}

impl ScriptedBackend {
    pub fn new(read_only: bool) -> Self {
        Self {
            caps: Capabilities { demo: false, read_only },
            held: Vec::new(),
            log: Vec::new(),
            snapshot: None,
            world: DemoBackend::new(World::demo()),
            ready: VecDeque::new(),
        }
    }

    /// The world's answer to `request`.
    fn answer_of(&mut self, request: Request) -> Envelope {
        self.log.push(request.clone());
        if let (slakio_core::backend::Command::Boot, Some(s)) = (&request.command, &self.snapshot) {
            let event = slakio_core::backend::Event::Booted(Box::new(s.clone()));
            return Envelope { generation: request.id, event };
        }
        self.world.send(request);
        self.world.poll().expect("the world answers at once")
    }

    /// Answer the held request `i` (its index in `held`) now.
    pub fn answer(&mut self, i: usize) -> Envelope {
        let e = self.reply(i);
        self.ready.push_back(e.clone());
        e
    }

    /// The world's answer to the held request `i`, carried out but not handed over: the test
    /// may change it, then [`ScriptedBackend::push`] it.
    pub fn reply(&mut self, i: usize) -> Envelope {
        let r = self.held.remove(i);
        self.answer_of(r)
    }

    /// Fail the held request `i` with `kind`.
    pub fn fail(&mut self, i: usize, kind: FailKind) {
        let r = self.held.remove(i);
        self.ready.push_back(Envelope::failed(r.id, kind, "scripted"));
    }

    /// Hand `envelope` to the app (again).
    pub fn push(&mut self, envelope: Envelope) {
        self.ready.push_back(envelope);
    }
}

impl Backend for ScriptedBackend {
    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn send(&mut self, request: Request) {
        match refuse_change(&request).filter(|_| self.caps.read_only) {
            Some(refused) => self.ready.push_back(refused),
            None => self.held.push(request),
        }
    }

    fn poll(&mut self) -> Option<Envelope> {
        self.ready.pop_front()
    }
}

/// The app over a scripted backend, with the demo harness's frames.
pub struct Scripted {
    pub d: Demo,
    pub b: ScriptedBackend,
}

impl Scripted {
    /// Connected and booted (the boot answered from the world, or from `snapshot`).
    pub fn booted(read_only: bool, snapshot: Option<Snapshot>) -> Self {
        let mut s = Self::connected(read_only);
        s.b.snapshot = snapshot;
        s.pump();
        s.b.answer(0);
        s.pump();
        s
    }

    /// Connected; the boot request not sent yet.
    pub fn connected(read_only: bool) -> Self {
        let b = ScriptedBackend::new(read_only);
        let mut app = App::new(Lang::En, Theme::terminal());
        app.resize(120, 40);
        app.connect(b.capabilities());
        let d = Demo { app, backend: DemoBackend::new(World::demo()), now: std::time::Instant::now() };
        Self { d, b }
    }

    /// One turn of the loop for the backend; `true` when the screen changed.
    pub fn pump(&mut self) -> bool {
        slakio_tui::exchange::exchange(&mut self.d.app, &mut self.b, self.d.now)
    }

    /// Press keys (config notation), then one turn of the loop.
    pub fn keys(&mut self, notation: &str) {
        for k in parse_keys(notation).unwrap() {
            self.d.app.handle_event(ratatui::crossterm::event::Event::Key(k.to_event()), self.d.now);
        }
        self.pump();
    }
}
