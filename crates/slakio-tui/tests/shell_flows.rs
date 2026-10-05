//! The shell over the demo world, driven headless: keys and mouse events in, frames out. The
//! demo backend is pumped by hand, the way the binary's loop does it. Screens are insta
//! snapshots (English only); under CI insta never rewrites them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use slakio_core::backend::{Backend, Envelope, Event as BackendEvent, Generation};
use slakio_core::i18n::Lang;
use slakio_tui::app::shell::{Region, View};
use slakio_tui::app::{App, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::keymap::parse_keys;
use slakio_tui::theme::Theme;
use slakio_tui::ui;
use slakio_world::World;
use std::time::Instant;

struct Demo {
    app: App,
    backend: DemoBackend,
    now: Instant,
}

impl Demo {
    fn new(w: u16, h: u16) -> Self {
        Self::with(w, h, Lang::En, Settings::default())
    }

    fn with(w: u16, h: u16, lang: Lang, settings: Settings) -> Self {
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
    fn pump(&mut self) {
        for (g, c) in self.app.take_commands() {
            self.backend.send(g, c);
        }
        while let Some(e) = self.backend.poll() {
            self.app.on_backend(e);
        }
    }

    /// Press keys written in config notation (`space w h`, `G`, `ctrl+l`).
    fn keys(&mut self, notation: &str) {
        for k in parse_keys(notation).unwrap() {
            self.app.handle_event(Event::Key(k.to_event()), self.now);
        }
        self.pump();
    }

    fn command(&mut self, cmd: &str) {
        self.keys(":");
        for c in cmd.chars() {
            self.keys(&c.to_string());
        }
        self.keys("enter");
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) -> bool {
        let ev = MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE };
        self.app.handle_event(Event::Mouse(ev), self.now)
    }

    fn screen(&self) -> String {
        let (w, h) = (self.app.size.width, self.app.size.height);
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| ui::draw(f, &self.app, self.now)).unwrap();
        let buf = t.backend().buffer();
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

    fn status_line(&self) -> String {
        self.screen().lines().last().unwrap().to_string()
    }
}

#[test]
fn the_demo_opens_on_home_of_the_first_workspace_at_three_sizes() {
    for (w, h) in [(80, 24), (120, 40), (200, 50)] {
        let d = Demo::new(w, h);
        let s = d.screen();
        assert!(s.contains("Favorites") && s.contains("# backend") && s.contains("A company"), "{s}");
        assert!(d.status_line().contains("demo"), "the status line says the data is invented");
        insta::assert_snapshot!(format!("demo_home_{w}x{h}"), s);
    }
}

#[test]
fn the_list_says_loading_until_the_backend_answers() {
    let mut app = App::new(Lang::En, Theme::terminal());
    app.resize(80, 24);
    let backend = DemoBackend::new(World::demo());
    app.connect(backend.capabilities());
    let d = Demo { app, backend, now: Instant::now() };
    assert!(d.screen().contains("Loading"), "{}", d.screen());
}

#[test]
fn an_answer_to_an_older_boot_request_is_dropped() {
    let mut d = Demo::new(80, 24);
    let stale = Envelope { generation: Generation(0), event: BackendEvent::Booted(Box::default()) };
    assert!(!d.app.on_backend(stale), "nothing changes");
    assert_eq!(d.app.model.workspaces().len(), 2, "the newer answer stays");
    // A second connect asks again; the first answer, arriving late, is dropped too.
    let first = d.app.take_commands();
    assert!(first.is_empty());
    let caps = d.backend.capabilities();
    d.app.connect(caps);
    d.app.connect(caps);
    let asked = d.app.take_commands();
    assert_eq!(asked.len(), 2);
    let old = Envelope { generation: asked[0].0, event: BackendEvent::Booted(Box::default()) };
    assert!(!d.app.on_backend(old));
    assert_eq!(d.app.model.workspaces().len(), 2);
}

#[test]
fn the_focused_rail_expands_over_the_list_or_pushes_it_aside() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+h");
    assert_eq!(d.app.shell.focus, Region::Rail);
    let s = d.screen();
    assert!(s.contains("A company") && s.contains("Home") && s.contains("Activity"), "{s}");
    insta::assert_snapshot!("demo_rail_overlay_120x40", s);
    let mut push = Demo::with(120, 40, Lang::En, Settings { rail_push: true, ..Settings::default() });
    push.keys("space w h");
    insta::assert_snapshot!("demo_rail_push_120x40", push.screen());
}

#[test]
fn the_rail_switches_workspace_and_view() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+h j enter");
    assert_eq!((d.app.shell.workspace, d.app.shell.focus), (1, Region::List));
    assert!(d.status_line().contains("B side"), "{}", d.status_line());
    d.keys("space d");
    assert_eq!(d.app.shell.view, View::Dms);
    let s = d.screen();
    assert!(s.contains(" DMs ") && s.contains("@ "), "{s}");
    assert!(!s.contains("# general"), "DMs lists no channels");
    insta::assert_snapshot!("demo_dms_120x40", s);
}

#[test]
fn views_of_a_later_version_say_so_and_show_no_invented_rows() {
    let mut d = Demo::new(120, 40);
    for (keys, view) in [("space a", View::Activity), ("space f", View::Files), ("space l", View::Later)] {
        d.keys(keys);
        assert_eq!(d.app.shell.view, view);
        let s = d.screen();
        assert!(s.contains("Available in a later"), "{s}");
        assert!(!s.contains("# "), "{s}");
    }
    d.command("home");
    assert_eq!(d.app.shell.view, View::Home);
}

#[test]
fn enter_opens_a_conversation_in_the_work_area_and_the_breadcrumb() {
    let mut d = Demo::new(120, 40);
    let s = d.screen();
    assert!(s.contains("Choose a conversation in the list and press Enter."), "{s}");
    d.keys("j enter");
    let s = d.screen();
    assert!(s.contains("▌#backend") && s.contains("Messages are not shown in this build yet."), "{s}");
    assert!(d.status_line().contains("#backend"), "{}", d.status_line());
    insta::assert_snapshot!("demo_open_120x40", s);
}

#[test]
fn list_keys_move_jump_and_fold() {
    let mut d = Demo::new(120, 40);
    let rows = d.app.shell.rows(&d.app.model).len();
    d.keys("G");
    assert_eq!(d.app.shell.list_cursor, rows - 1);
    d.keys("g g");
    assert_eq!(d.app.shell.list_cursor, 0);
    d.keys("enter");
    assert!(d.screen().contains("▸ Favorites"), "folded");
    assert_eq!(d.app.shell.rows(&d.app.model).len(), rows - 2);
    d.keys("down down up");
    assert_eq!(d.app.shell.list_cursor, 1);
}

#[test]
fn a_leader_sequence_shows_its_keys_until_it_ends() {
    let mut d = Demo::new(120, 40);
    d.keys("space w");
    assert!(d.status_line().contains("Space w"), "{}", d.status_line());
    assert_eq!(d.app.shell.focus, Region::List);
    d.keys("h");
    assert_eq!(d.app.shell.focus, Region::Rail);
    assert!(!d.status_line().contains("Space w"));
    d.keys("space w l ctrl+l");
    assert_eq!(d.app.shell.focus, Region::Work);
    // A sequence nothing is bound to is dropped without doing anything.
    d.keys("space x");
    assert_eq!(d.app.shell.focus, Region::Work);
    assert!(d.app.keys.pending().is_empty());
}

#[test]
fn the_list_panel_hides_and_comes_back() {
    let mut d = Demo::new(120, 40);
    d.keys("space e");
    assert!(d.app.shell.list_hidden);
    assert!(!d.screen().contains("Favorites"));
    d.command("list");
    assert!(d.screen().contains("Favorites"));
}

#[test]
fn hovering_the_rail_expands_it_and_clicks_select_and_open() {
    let mut d = Demo::new(120, 40);
    assert!(d.mouse(MouseEventKind::Moved, 1, 5), "entering the rail redraws");
    assert!(d.app.shell.rail_expanded());
    assert!(!d.mouse(MouseEventKind::Moved, 2, 6), "moving inside it does not");
    assert!(d.mouse(MouseEventKind::Moved, 60, 6));
    assert!(!d.app.shell.rail_expanded());
    // Row 2 of the rail is workspace B (row 0 is the border).
    d.mouse(MouseEventKind::Down(MouseButton::Left), 1, 2);
    assert_eq!(d.app.shell.workspace, 1);
    // The separator does nothing.
    d.mouse(MouseEventKind::Down(MouseButton::Left), 1, 3);
    assert_eq!(d.app.shell.workspace, 1);
    // A list row opens it: row 2 is the first channel under the first section.
    let list = d.app.areas().list.unwrap();
    d.mouse(MouseEventKind::Down(MouseButton::Left), list.x + 3, 2);
    let open = d.app.shell.open.clone().expect("opened");
    assert_eq!(d.app.model.target(&open).unwrap().workspace.as_str(), "TDEMOB");
    d.mouse(MouseEventKind::Down(MouseButton::Left), 100, 10);
    assert_eq!(d.app.shell.focus, Region::Work);
}

#[test]
fn a_terminal_too_small_says_how_much_room_it_needs() {
    let d = Demo::new(40, 8);
    let s = d.screen();
    assert!(s.contains("40×8") && s.contains("50×10"), "{s}");
    for (w, h) in [(0, 0), (1, 1), (49, 30), (120, 9), (50, 10)] {
        let mut d = Demo::new(w, h);
        d.keys("ctrl+h j enter space e");
        let _ = d.screen();
    }
}

#[test]
fn the_korean_ui_translates_the_shell() {
    let d = Demo::with(120, 40, Lang::Ko, Settings::default());
    let s = d.screen();
    // The list title "Home" in Korean, and the demo badge.
    assert!(s.contains("\u{D648}") && d.status_line().contains("\u{B370}\u{BAA8}"), "{s}");
    assert!(!s.contains(" Home "), "{s}");
}

#[test]
fn icons_replace_the_rail_letters() {
    let d = Demo::with(120, 40, Lang::En, Settings { icons: true, ..Settings::default() });
    let s = d.screen();
    assert!(s.contains('\u{F02DC}'), "the Home icon: {s}");
}

#[test]
fn the_overlay_rail_hides_the_list_panel_it_covers() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+h");
    let a = d.app.areas();
    let list = a.list.expect("the list panel is shown");
    assert!(a.rail.right() < list.right(), "the rail covers part of the list panel");
    let s = d.screen();
    // Between the overlay and the work area nothing of the list panel shows: no border pieces
    // (`╮──────╮`), no fragments of names or counts.
    for (y, line) in s.lines().take(usize::from(a.work.height)).enumerate() {
        let cells: Vec<char> = line.chars().collect();
        let gap: String = cells
            .get(usize::from(a.rail.right())..usize::from(a.work.x))
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        assert!(gap.trim().is_empty(), "row {y}: {gap:?} shows through\n{s}");
    }
    // The screen itself: the snapshot `demo_rail_overlay_120x40`.
}
