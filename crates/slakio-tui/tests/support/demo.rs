//! The headless demo the flow tests drive: the app over the demo backend, pumped by hand the
//! way the binary's loop does it, keys in config notation, frames as text. Shared by the flow
//! test files (`#[path]`), each of which uses part of it.

#![allow(dead_code)]

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
use slakio_core::backend::Backend;
use slakio_core::i18n::Lang;
use slakio_tui::app::{App, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::keymap::parse_keys;
use slakio_tui::theme::Theme;
use slakio_tui::ui;
use slakio_world::World;
use std::time::Instant;

pub struct Demo {
    pub app: App,
    pub backend: DemoBackend,
    pub now: Instant,
}

impl Demo {
    pub fn new(w: u16, h: u16) -> Self {
        Self::with(w, h, Lang::En, Settings::default())
    }

    pub fn with(w: u16, h: u16, lang: Lang, settings: Settings) -> Self {
        let mut app = App::new(lang, Theme::terminal());
        app.settings = settings;
        app.resize(w, h);
        let backend = DemoBackend::new(World::demo());
        app.connect(backend.capabilities());
        let mut d = Self { app, backend, now: Instant::now() };
        d.pump();
        d
    }

    /// One turn of the binary's loop for the backend.
    pub fn pump(&mut self) {
        slakio_tui::exchange::exchange(&mut self.app, &mut self.backend);
    }

    /// Press keys written in config notation (`space w h`, `G`, `ctrl+l`).
    pub fn keys(&mut self, notation: &str) {
        for k in parse_keys(notation).unwrap() {
            self.app.handle_event(Event::Key(k.to_event()), self.now);
        }
        self.pump();
    }

    pub fn command(&mut self, cmd: &str) {
        self.keys(":");
        self.type_text(cmd);
        self.keys("enter");
    }

    pub fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) -> bool {
        let ev = MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };
        self.app.handle_event(Event::Mouse(ev), self.now)
    }

    /// The frame as drawn: every cell with its style.
    pub fn buffer(&self) -> ratatui::buffer::Buffer {
        let (w, h) = (self.app.size.width, self.app.size.height);
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| ui::draw(f, &self.app, self.now)).unwrap();
        t.backend().buffer().clone()
    }

    pub fn screen(&self) -> String {
        let (w, h) = (self.app.size.width, self.app.size.height);
        let buf = &self.buffer();
        (0..h)
            .map(|y| {
                // A wide character fills two cells; the second one is not text of its own.
                let mut line = String::new();
                let mut x = 0;
                while x < w {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    x += (ratatui::text::Span::raw(sym).width() as u16).max(1);
                }
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The screen for a snapshot file: Hangul (the demo has Korean names and text) becomes a
    /// full-width `＊` of the same width, since tracked files hold no Hangul.
    pub fn snap(&self) -> String {
        mask_hangul(&self.screen())
    }

    pub fn status_line(&self) -> String {
        self.screen().lines().last().unwrap().to_string()
    }
}

/// Hangul syllables and jamo as `＊` (two cells, like them); other Hangul code points as `?`.
pub fn mask_hangul(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{AC00}'..='\u{D7A3}' | '\u{3130}'..='\u{318F}' | '\u{1100}'..='\u{115F}' => '\u{FF0A}',
            '\u{1160}'..='\u{11FF}' | '\u{A960}'..='\u{A97F}' | '\u{D7B0}'..='\u{D7FF}' => '?',
            c => c,
        })
        .collect()
}

/// A character that must never reach a cell: controls, bidi, zero-width (a joiner inside an
/// emoji sequence is part of the picture).
pub fn harmful(c: char) -> bool {
    c.is_control()
        || ['\u{200B}', '\u{200C}', '\u{200E}', '\u{200F}'].contains(&c)
        || ('\u{202A}'..='\u{202E}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || c == '\u{FEFF}'
}

/// The screen holds no character a terminal would act on.
pub fn assert_harmless(screen: &str) {
    for (y, line) in screen.lines().enumerate() {
        assert!(!line.chars().any(harmful), "row {y}: {line:?}");
    }
}

impl Demo {
    /// Open the conversation of the shown workspace named `name` from the list panel (Home).
    pub fn open(&mut self, name: &str) {
        use slakio_tui::app::model::Row;
        use slakio_tui::app::shell::{Region, View};
        self.app.shell.view = View::Home;
        self.app.shell.collapsed.clear();
        let rows = self.app.shell.rows(&self.app.model);
        let at = rows
            .iter()
            .position(|r| matches!(r, Row::Conversation(i) if self.app.model.conversation(*i).name == name))
            .unwrap_or_else(|| panic!("no conversation {name}"));
        self.app.shell.focus = Region::List;
        self.app.shell.list_cursor = at;
        self.keys("enter");
    }

    /// Type text as key presses (what an IME's commits arrive as).
    pub fn type_text(&mut self, text: &str) {
        use ratatui::crossterm::event::{KeyCode, KeyEvent};
        for c in text.chars() {
            let ev = Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            self.app.handle_event(ev, self.now);
        }
        self.pump();
    }

    /// The bytes the real terminal backend writes for the current frame.
    pub fn terminal_bytes(&self) -> Vec<u8> {
        use ratatui::backend::CrosstermBackend;
        use ratatui::layout::Rect;
        use ratatui::{TerminalOptions, Viewport};
        let (w, h) = (self.app.size.width, self.app.size.height);
        let options = TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, w, h)) };
        let out = Shared::default();
        let mut t = Terminal::with_options(CrosstermBackend::new(out.clone()), options).unwrap();
        t.draw(|f| ui::draw(f, &self.app, self.now)).unwrap();
        out.0.borrow().clone()
    }
}

/// A writer whose bytes the test reads after the backend wrote them.
#[derive(Clone, Default)]
struct Shared(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

impl std::io::Write for Shared {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
