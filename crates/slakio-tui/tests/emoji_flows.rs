//! A workspace's own emoji: once a frame shows a shortcode the standard emoji do not know, the
//! workspace's list is asked for once; then each emoji that is drawn is asked for as a picture
//! and shown as two placeholder cells in the text and the reaction chips (a line never breaks
//! inside one, and it takes two cells, not its name's); an alias of a standard emoji is that
//! emoji. Pictures off, not here yet, failed, refused (another host, a cycle): `:name:` stays.
//! Only what is shown asks for a picture; scrolled away or `:images off`, its cells are gone.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, FailKind, Request};
use slakio_core::images::Source;
use slakio_core::model::{Message, Reaction, Ts, WorkspaceId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Effect;
use slakio_tui::images::graphics::is_placeholder;
use slakio_tui::images::prepare;

/// The white check mark: what the world's `:yes:` (an alias) draws.
const CHECK: &str = "\u{2705}";

/// Booted, `deploys` open at `w`×`h` with `messages` (text, reactions), oldest first, pictures
/// `on` (as the binary sets them in kitty) or off; its workspace.
fn shown(messages: &[(&str, &[&str])], w: u16, h: u16, on: bool) -> (Scripted, WorkspaceId) {
    let mut s = Scripted::booted(true, None);
    if on {
        s.d.app.images.env = slakio_tui::images::detect::Env { term: "xterm-kitty".into(), ..Default::default() };
        (s.d.app.images.setting, s.d.app.images.tty) = ("auto".into(), true);
        s.d.app.images.resolve();
        assert!(s.d.app.images.on);
    }
    s.d.app.resize(w, h);
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    let mut e = s.b.reply(0);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let ws = page.target.workspace().clone();
    let author = page.messages[0].user.clone();
    page.messages = (messages.iter().enumerate())
        .map(|(k, (text, reactions))| Message {
            ts: Ts(1_767_603_600_000_000 + k as u64 * 3_600_000_000),
            user: author.clone(),
            text: Remote::new(*text),
            thread: None,
            reactions: reactions.iter().map(|r| Reaction { name: Remote::new(*r), count: 2, mine: false }).collect(),
            edited: false,
            files: Vec::new(),
        })
        .collect();
    page.complete = true;
    s.b.push(e);
    s.pump();
    (s, ws)
}

/// Draw a frame and ask for what it wanted, as the binary's loop does: the lists and pictures
/// asked for (taken from those the backend holds).
fn frame(s: &mut Scripted) -> Vec<Request> {
    s.d.screen();
    s.d.app.ask_images(s.d.now);
    s.pump();
    let (asked, kept) = s.b.held.drain(..).partition(|r| matches!(r.command, Command::Image { .. } | Command::Emoji));
    s.b.held = kept;
    asked
}

fn lists(asked: &[Request]) -> Vec<&Request> {
    asked.iter().filter(|r| r.command == Command::Emoji).collect()
}

fn pictures(asked: &[Request]) -> Vec<Source> {
    (asked.iter())
        .filter_map(|r| match &r.command {
            Command::Image { source, .. } => Some(source.clone()),
            _ => None,
        })
        .collect()
}

/// Answer list request `r` as the world's backend does.
fn give_list(s: &mut Scripted, r: &Request) {
    s.b.held.push(r.clone());
    let i = s.b.held.len() - 1;
    s.b.answer(i);
    s.pump();
}

/// Answer picture request `r` with an emoji's picture and decode it as the binary does.
fn give_picture(s: &mut Scripted, r: &Request) {
    s.b.push(Envelope { generation: r.id, event: Event::Image { bytes: slakio_world::pictures::png("e", 64, 64) } });
    s.pump();
    for effect in s.d.app.take_effects() {
        if let Effect::Decode(job) = effect {
            let result = prepare(&job, Some((9, 18)));
            s.d.app.images.decoded((job.source, job.request), result);
        }
    }
}

fn emoji(ws: &WorkspaceId, name: &str) -> Source {
    Source::Emoji(slakio_world::emoji::picture(ws.as_str(), name))
}

/// The list given, then each picture asked for given: what the screen shows then.
fn drawn(s: &mut Scripted) {
    let asked = frame(s);
    for r in lists(&asked) {
        give_list(s, r);
    }
    for _ in 0..3 {
        let asked = frame(s);
        for r in asked.iter().filter(|r| matches!(r.command, Command::Image { .. })) {
            give_picture(s, r);
        }
    }
}

/// Each row's placeholder cells: (row, their columns).
fn cells(s: &Scripted) -> Vec<(u16, Vec<u16>)> {
    let buf = s.d.buffer();
    let area = buf.area;
    (0..area.height)
        .map(|y| (y, (0..area.width).filter(|&x| is_placeholder(buf[(x, y)].symbol())).collect::<Vec<_>>()))
        .filter(|(_, xs)| !xs.is_empty())
        .collect()
}

#[test]
fn the_list_is_asked_once_then_the_picture_and_it_is_drawn_in_two_cells() {
    let (mut s, ws) = shown(&[("ship it :party-blob: now :yes:", &[])], 120, 40, true);
    let asked = frame(&mut s);
    assert_eq!(lists(&asked).len(), 1, "the list, once");
    assert_eq!(lists(&asked)[0].workspace.as_ref(), Some(&ws));
    assert!(pictures(&asked).is_empty(), "no picture before the list says where it is");
    assert!(lists(&frame(&mut s)).is_empty(), "not asked again while it comes");
    give_list(&mut s, lists(&asked)[0]);
    let asked = frame(&mut s);
    assert_eq!(pictures(&asked), [emoji(&ws, "party-blob")]);
    assert!(s.d.screen().contains(":party-blob:"), "text until it comes");
    give_picture(&mut s, &asked[0]);
    assert!(s.d.app.images.take_graphics().starts_with("\x1b_Ga=T,U=1,f=100"), "sent once");
    let screen = s.d.screen();
    assert!(!screen.contains(":party-blob:"), "{screen}");
    assert!(screen.contains(CHECK), "an alias of a standard emoji is that emoji:\n{screen}");
    let rows = cells(&s);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let y = rows[0].0;
    let line = screen.lines().nth(usize::from(y)).unwrap();
    assert!(line.contains("ship it") && line.contains("now"), "{line}");
    assert_eq!(rows[0].1.len(), 2, "two cells");
    assert_eq!(rows[0].1[1], rows[0].1[0] + 1, "side by side");
    // Two cells, not its name's twelve: `now` starts one blank after them.
    let buf = s.d.buffer();
    let after = rows[0].1[1] + 2;
    let next: String = (after..after + 3).map(|x| buf[(x, y)].symbol().to_string()).collect();
    assert_eq!(next, "now");
    assert!(frame(&mut s).is_empty(), "never asked again");
}

#[test]
fn a_reaction_of_the_workspace_s_own_is_its_picture_in_the_chip() {
    let (mut s, ws) = shown(&[("hello", &["party-blob", "yes", "far-away"])], 120, 40, true);
    drawn(&mut s);
    let rows = cells(&s);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].1.len(), 2);
    let screen = s.d.screen();
    let line = screen.lines().nth(usize::from(rows[0].0)).unwrap();
    assert!(line.contains(&format!(" {CHECK} 2 ")), "{line}");
    assert!(line.contains(" :far-away: 2 "), "another host: text\n{line}");
    assert!(!line.contains("party-blob"), "{line}");
    assert!(s.d.app.images.held().0 == 1);
    let _ = ws;
}

#[test]
fn a_line_never_breaks_inside_an_emoji_and_it_takes_two_cells() {
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    for w in 64..=100u16 {
        let text = format!("{words} :party-blob: {words} :ship-it:. {words}");
        let (mut s, _) = shown(&[(text.as_str(), &[])], w, 40, true);
        drawn(&mut s);
        let rows = cells(&s);
        assert_eq!(rows.iter().map(|(_, xs)| xs.len()).sum::<usize>(), 4, "{w}: {rows:?}");
        for (y, xs) in &rows {
            assert_eq!(xs.len(), 2, "{w}: row {y} holds one emoji whole: {rows:?}");
            assert_eq!(xs[1], xs[0] + 1, "{w}: {rows:?}");
        }
        let screen = s.d.screen();
        let shown_words: Vec<&str> = screen.split_whitespace().filter(|t| words.contains(*t)).collect();
        assert!(shown_words.len() >= 36, "{w}: no word lost:\n{screen}");
        assert!(!screen.lines().any(|l| l.trim_start().starts_with('.')), "{w}: the period stays:\n{screen}");
    } // Written against a word too long for a line, it still moves whole.
    for n in 20..=70 {
        let text = format!("{}:party-blob:{}", "x".repeat(n), "y".repeat(n));
        let (mut s, _) = shown(&[(text.as_str(), &[])], 80, 40, true);
        drawn(&mut s);
        let rows = cells(&s);
        assert_eq!(rows.len(), 1, "{n}: {rows:?}");
        assert_eq!(rows[0].1.len(), 2, "{n}: {rows:?}");
    }
}

#[test]
fn pictures_off_the_name_stays_and_no_picture_is_asked() {
    let (mut s, _) = shown(&[("ship it :party-blob: :yes:", &["party-blob"])], 120, 40, false);
    let asked = frame(&mut s);
    assert_eq!(lists(&asked).len(), 1, "the list still: an alias of a standard emoji draws it");
    give_list(&mut s, lists(&asked)[0]);
    assert!(frame(&mut s).is_empty(), "no picture with pictures off");
    let screen = s.d.screen();
    assert!(screen.contains(":party-blob:") && screen.contains(" :party-blob: 2 "), "{screen}");
    assert!(screen.contains(CHECK), "{screen}");
    assert!(cells(&s).is_empty());
    assert_eq!(s.d.app.images.take_graphics(), "");
}

#[test]
fn an_emoji_refused_or_failed_stays_text_and_nothing_is_said() {
    let (mut s, ws) = shown(&[(":far-away: :plain: :loop-a: :party-blob:", &[])], 120, 40, true);
    let status = s.d.status_line();
    let asked = frame(&mut s);
    give_list(&mut s, lists(&asked)[0]);
    let asked = frame(&mut s);
    assert_eq!(pictures(&asked), [emoji(&ws, "party-blob")], "another host, http, a cycle: never asked");
    s.b.push(Envelope::failed(asked[0].id, FailKind::Api { code: "file_not_available".into() }, "gone"));
    s.pump();
    let screen = s.d.screen();
    for name in [":far-away:", ":plain:", ":loop-a:", ":party-blob:"] {
        assert!(screen.contains(name), "{name}\n{screen}");
    }
    assert!(cells(&s).is_empty());
    assert_eq!(s.d.status_line(), status, "never in the status line");
    assert!(frame(&mut s).is_empty(), "a picture gone is not asked again");
}

#[test]
fn a_list_that_failed_leaves_text_unsaid_and_is_asked_again_after_its_wait() {
    let (mut s, _) = shown(&[("ship :party-blob:", &[])], 120, 40, true);
    let status = s.d.status_line();
    let asked = frame(&mut s);
    s.b.held.push(lists(&asked)[0].clone());
    s.b.fail(s.b.held.len() - 1, FailKind::Network);
    s.pump();
    assert_eq!(s.d.status_line(), status, "never in the status line");
    assert!(s.d.screen().contains(":party-blob:"));
    assert!(frame(&mut s).is_empty(), "not again before its wait");
    s.d.now += slakio_tui::images::RETRY_AFTER * 2;
    let asked = frame(&mut s);
    assert_eq!(lists(&asked).len(), 1, "asked again once its wait is over");
    s.b.push(Envelope::failed(lists(&asked)[0].id, FailKind::Api { code: "missing_scope".into() }, "no"));
    s.pump();
    s.d.now += slakio_tui::images::RETRY_AFTER * 2;
    assert!(frame(&mut s).is_empty(), "refused: never again this run");
}

#[test]
fn text_without_the_workspace_s_own_emoji_asks_for_nothing() {
    let (mut s, _) = shown(&[("at 10:30:00 :+1: :eyes: :thinking_face: plain", &["+1", "tada"])], 120, 40, true);
    assert!(frame(&mut s).is_empty());
}

#[test]
fn scrolled_away_or_images_off_the_cells_go_and_only_what_is_shown_is_asked() {
    let mut texts: Vec<String> = vec!["yankee :ship-it: up here".to_string()];
    texts.extend((0..30).map(|k| format!("filler {k}")));
    texts.push("zulu :party-blob: here".to_string());
    let messages: Vec<(&str, &[&str])> = texts.iter().map(|t| (t.as_str(), &[][..])).collect();
    let (mut s, ws) = shown(&messages, 100, 20, true);
    drawn(&mut s);
    assert!(!s.d.screen().contains("yankee"), "the oldest is off screen");
    assert_eq!(s.d.app.images.held().0, 1, "only the one shown was asked and held");
    assert_eq!(cells(&s).len(), 1);
    // Scrolled to the top: its cells go, the one now shown is asked.
    s.keys("g g");
    let screen = s.d.screen();
    assert!(screen.contains("yankee") && !screen.contains("zulu"), "{screen}");
    assert!(cells(&s).is_empty(), "nothing of it left");
    let asked = frame(&mut s);
    assert_eq!(pictures(&asked), [emoji(&ws, "ship-it")]);
    give_picture(&mut s, &asked[0]);
    assert_eq!(cells(&s).len(), 1);
    // `:images off`: every picture leaves at once, the names are text again.
    s.d.app.images.take_graphics();
    s.d.app.images.choose("off");
    let sent = s.d.app.images.take_graphics();
    assert!(sent.contains("a=d,d=I") && sent.ends_with("\x1b_Ga=d,d=A,q=2\x1b\\"), "{sent:?}");
    assert!(cells(&s).is_empty());
    assert!(s.d.screen().contains(":ship-it:"));
    assert!(pictures(&frame(&mut s)).is_empty());
}

#[test]
fn more_emoji_than_the_terminal_holds_ask_a_frame_s_share_once_each_and_the_rest_stay_text() {
    use slakio_core::images::emoji::Custom;
    use slakio_tui::images::EMOJI_PER_FRAME;
    let walls: Vec<String> = (0..200).map(|k| format!("wall-{k}")).collect();
    let chips: Vec<String> = (0..65).map(|k| format!("chip-{k}")).collect();
    let wall_text = walls.iter().map(|n| format!(":{n}:")).collect::<Vec<_>>().join(" ");
    let chip_names: Vec<&str> = chips.iter().map(String::as_str).collect();
    for (text, last) in [(wall_text.as_str(), "wall-199"), ("hello", "chip-64")] {
        let (mut s, ws) = shown(&[(text, &chip_names)], 200, 50, true);
        let asked = frame(&mut s);
        let list = lists(&asked)[0].clone();
        let emoji = (walls.iter().chain(&chips))
            .map(|n| (n.clone(), Custom::Picture(slakio_world::emoji::picture(ws.as_str(), n))))
            .collect();
        s.b.push(Envelope { generation: list.id, event: Event::Emoji { emoji } });
        s.pump();
        let mut all: Vec<Source> = Vec::new();
        for _ in 0..20 {
            for r in frame(&mut s) {
                all.extend(pictures(std::slice::from_ref(&r)));
                give_picture(&mut s, &r);
            }
        }
        let distinct: std::collections::HashSet<&Source> = all.iter().collect();
        assert_eq!(distinct.len(), all.len(), "{last}: an emoji on screen was asked for again");
        assert_eq!(all.len(), EMOJI_PER_FRAME, "{last}: a frame's share, no more");
        assert!(frame(&mut s).is_empty(), "{last}: settled");
        assert_eq!(cells(&s).iter().map(|(_, xs)| xs.len()).sum::<usize>(), 2 * EMOJI_PER_FRAME, "{last}");
        let screen = s.d.screen();
        assert!(screen.contains(&format!(":{last}:")), "{last}: past the share, text:\n{screen}");
    }
}
