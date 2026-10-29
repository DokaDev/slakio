//! `:net off` and `:net on` in the app: off says so, marks the status line `Offline` until it is
//! on again and asks the binary to switch the network off; on asks it to switch it on and asks
//! again for the workspaces that could not start meanwhile. A network off for the whole run stays
//! off (`:net on` says why), and so does one the binary could not switch on. The demo has no
//! `:net`. At 120x40 and 80x24.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use scripted::Scripted;
use slakio_core::backend::{Command, FailKind};
use slakio_tui::app::Effect;

/// Run `:text` in the palette.
fn command(s: &mut Scripted, text: &str) {
    s.keys(":");
    for c in text.chars() {
        s.d.app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), s.d.now);
    }
    s.keys("enter");
}

fn switched(s: &mut Scripted) -> Vec<bool> {
    s.d.app
        .take_effects()
        .into_iter()
        .filter_map(|e| if let Effect::Network(on) = e { Some(on) } else { None })
        .collect()
}

#[test]
fn off_says_so_marks_the_status_line_and_on_takes_it_back() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut s = Scripted::booted(true, None);
        s.d.app.resize(w, h);
        assert!(!s.d.status_line().contains("Offline"));
        command(&mut s, "net off");
        assert_eq!(switched(&mut s), [false]);
        let line = s.d.status_line();
        assert!(line.contains("Network off: nothing is sent") && line.contains("Offline"), "{w}x{h}: {line}");
        if (w, h) == (80, 24) {
            insta::assert_snapshot!("net_offline_80x24", s.d.snap());
        }
        // Long after the notice went, still said.
        for _ in 0..12 {
            s.d.now += std::time::Duration::from_secs(10);
            s.d.app.on_tick(s.d.now);
        }
        assert!(s.d.status_line().contains("Offline"), "{w}x{h}: {}", s.d.status_line());
        // The palette marks the value in use.
        s.keys(":");
        s.d.app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)), s.d.now);
        for c in "et ".chars() {
            s.d.app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), s.d.now);
        }
        let rows = s.d.app.palette_rows();
        let off = rows.iter().find(|r| r.name == "off").expect("off listed");
        assert_eq!(off.keys, "current");
        s.keys("esc");
        command(&mut s, "net on");
        assert_eq!(switched(&mut s), [true]);
        let line = s.d.status_line();
        assert!(line.contains("Network on") && !line.contains("Offline"), "{w}x{h}: {line}");
        // `:net` alone switches it the other way.
        command(&mut s, "net");
        assert_eq!(switched(&mut s), [false]);
        command(&mut s, "net");
        assert_eq!(switched(&mut s), [true]);
    }
}

#[test]
fn on_again_asks_again_for_the_workspace_that_could_not_start_meanwhile() {
    let mut s = Scripted::connected(true);
    s.pump();
    command(&mut s, "net off");
    let i = s.b.held.iter().position(|r| r.command == Command::Boot).unwrap();
    s.b.fail(i, FailKind::Offline);
    s.pump();
    assert!(!s.b.held.iter().any(|r| r.command == Command::Boot));
    command(&mut s, "net on");
    s.pump();
    assert!(s.b.held.iter().any(|r| r.command == Command::Boot), "booted again: {:?}", s.b.held);
}

#[test]
fn a_network_off_for_the_run_stays_off_and_says_why() {
    let mut s = Scripted::booted(true, None);
    s.d.app.model.net.locked = true;
    assert!(s.d.status_line().contains("Offline"), "{}", s.d.status_line());
    command(&mut s, "net on");
    assert!(switched(&mut s).is_empty(), "never switched on");
    let line = s.d.status_line();
    assert!(line.contains("Network off for this run: started offline") && line.contains("Offline"), "{line}");
    // Switched on, the binary could not (the run's request cap was reached): off for good.
    let mut s = Scripted::booted(true, None);
    command(&mut s, "net on");
    assert_eq!(switched(&mut s), [true]);
    s.d.app.net_switched(false, s.d.now);
    let line = s.d.status_line();
    assert!(line.contains("Request limit reached") && line.contains("Offline"), "{line}");
}

#[test]
fn the_demo_has_no_net() {
    let mut d = demo::Demo::new(120, 40);
    d.keys(":");
    d.type_text("net");
    assert!(d.app.palette_rows().iter().all(|r| !r.name.contains(":net")), "{:?}", d.app.palette_rows());
    d.keys("esc");
    d.command("net off");
    assert!(!d.app.take_effects().iter().any(|e| matches!(e, Effect::Network(_))));
    assert!(!d.status_line().contains("Offline"), "{}", d.status_line());
}
