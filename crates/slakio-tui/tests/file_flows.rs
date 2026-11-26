//! The files messages share: cards under the text (name, kind, size; one hosted elsewhere says
//! so); `f` downloads the one file of the selected message at once, or lists several to pick
//! (letters, `j`/`k` and `Enter`, a click, `Esc`); the status line says how much has come;
//! `F` (or `:download cancel`) stops what is on its way; a file on its way is not asked for
//! again, and two on their way are said as two. A saved file of a kind a viewer shows, marked as
//! from the internet, is offered to open (nothing opens until the user says yes; any other kind,
//! one that could not be marked and the demo's are only saved); a failure says why.
//! A read-only workspace downloads too (nothing changes in Slack). At 120x40 and 80x24, in
//! English and Korean; the demo backend saves the demo's files in a scratch folder.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use demo::{Demo, assert_harmless};
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, FailKind};
use slakio_core::i18n::{I18n, Label, Lang, Msg};
use slakio_core::model::{File, FileId, Message, Ts};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Effect;
use slakio_tui::app::dialog::Question;
use std::path::PathBuf;

const SIZES: [(Lang, u16, u16); 3] = [(Lang::En, 120, 40), (Lang::En, 80, 24), (Lang::Ko, 80, 24)];

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-file-flows-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn file(id: &str, name: &str, kind: &str, size: Option<u64>, download: bool) -> File {
    File {
        id: FileId::new(id),
        name: Remote::new(name),
        kind: Remote::new(kind),
        size,
        download: download.then(|| format!("https://{}/files-pri/T-{id}/download/x", slakio_core::model::FILE_HOST)),
        thumb: None,
    }
}

/// Read-only, booted, `deploys` open at `w`×`h` in `lang`, its newest message sharing `files`
/// and selected; downloads go to `dir`.
fn shown(files: Vec<File>, w: u16, h: u16, lang: Lang, dir: Option<PathBuf>) -> Scripted {
    shown_with("the files", files, w, h, lang, dir)
}

/// [`shown`], the message's text `text`.
fn shown_with(text: &str, files: Vec<File>, w: u16, h: u16, lang: Lang, dir: Option<PathBuf>) -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.app.i18n = I18n::new(lang);
    s.d.app.resize(w, h);
    s.d.app.downloads.dir = dir;
    s.d.app.downloads.max = 1024 * 1024 * 1024;
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    let mut e = s.b.reply(0);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let author = page.messages[0].user.clone();
    page.messages = vec![Message {
        ts: Ts(1_767_603_600_000_000),
        user: author,
        text: Remote::new(text),
        thread: None,
        reactions: Vec::new(),
        edited: false,
        emoji_pictures: Vec::new(),
        files,
    }];
    page.complete = true;
    s.b.push(e);
    s.pump();
    s.keys("G");
    s
}

/// The download the app asked for (held), as the backend got it.
fn asked(s: &Scripted) -> Option<(slakio_core::backend::Generation, File)> {
    s.b.held.iter().rev().find_map(|r| match &r.command {
        Command::Download { file, .. } => Some((r.id, file.clone())),
        _ => None,
    })
}

fn effects(s: &mut Scripted) -> Vec<Effect> {
    s.d.app.take_effects()
}

/// The answer of a download saved as `path`; `unmarked`: the system's mark could not be set.
fn saved(file: FileId, path: &std::path::Path, unmarked: bool) -> Event {
    Event::Downloaded { file, path: path.to_path_buf(), bytes: 10, unmarked }
}

#[test]
fn cards_show_each_file_under_the_text() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let i18n = I18n::new(lang);
        let files = vec![
            file("F1", "Q3-report.pdf", "PDF", Some(1_234_567), true),
            file("F2", "Shared plan", "Google Docs", None, false),
        ];
        let s = shown(files, w, h, lang, None);
        let screen = s.d.screen();
        let card = screen.lines().find(|l| l.contains("Q3-report.pdf")).unwrap_or_else(|| panic!("{at}:\n{screen}"));
        assert!(card.contains("PDF · 1.2 MB"), "{at}: {card}");
        let elsewhere = i18n.label(Label::FileElsewhere).to_string();
        let card = screen.lines().find(|l| l.contains("Shared plan")).unwrap_or_else(|| panic!("{at}:\n{screen}"));
        assert!(card.contains(&elsewhere), "{at}: {card}");
        assert!(screen.contains('\u{21E9}'), "{at}: a card's mark without icons");
        assert!(card.contains('\u{21D7}') && !card.contains('\u{21E9}'), "{at}: elsewhere, an arrow out: {card}");
    }
}

#[test]
fn f_downloads_the_one_file_and_offers_to_open_it_once_saved() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let i18n = I18n::new(lang);
        let dir = scratch("one");
        let mut s = shown(vec![file("F1", "Q3-report.pdf", "PDF", Some(2000), true)], w, h, lang, Some(dir.clone()));
        s.keys("f");
        let (id, f) = asked(&s).unwrap_or_else(|| panic!("{at}: a download asked for"));
        assert_eq!(f.id, FileId::new("F1"));
        let Some(Command::Download { dir: to, max, .. }) = s.b.held.last().map(|r| r.command.clone()) else { panic!() };
        assert_eq!((to, max), (dir.clone(), 1024 * 1024 * 1024));
        let stop = "F".to_string();
        let said = i18n.msg(&Msg::FileWaiting { name: "Q3-report.pdf".into(), stop: stop.clone() }).to_string();
        assert!(s.d.status_line().contains(&said), "{at}: {}", s.d.status_line());
        // Half of it came.
        s.b.push(Envelope {
            generation: id,
            event: Event::Progress { file: f.id.clone(), got: 1000, total: Some(2000) },
        });
        s.pump();
        let half =
            i18n.msg(&Msg::FileProgress { name: "Q3-report.pdf".into(), percent: "50".into(), stop }).to_string();
        assert!(s.d.status_line().contains(&half), "{at}: {}", s.d.status_line());
        // Saved (and marked from the internet as it was saved): asked about; nothing opens yet.
        let path = dir.join("Q3-report.pdf");
        s.b.held.retain(|r| r.id != id);
        s.b.push(Envelope { generation: id, event: saved(f.id, &path, false) });
        s.pump();
        assert_eq!(effects(&mut s), [], "{at}");
        assert_eq!(s.d.app.question(), Some(Question::File), "{at}");
        let screen = s.d.screen();
        assert!(screen.contains("Q3-report.pdf"), "{at}:\n{screen}");
        // The safe answer has the focus: Enter opens nothing.
        s.keys("enter");
        assert!(effects(&mut s).is_empty(), "{at}");
        // Asked again (another download), yes opens it.
        s.keys("f");
        let (id, f) = asked(&s).unwrap();
        s.b.held.retain(|r| r.id != id);
        s.b.push(Envelope { generation: id, event: saved(f.id, &path, false) });
        s.pump();
        effects(&mut s);
        s.keys("y");
        assert_eq!(effects(&mut s), [Effect::OpenFile(path)], "{at}");
        assert_eq!(s.b.log.iter().filter(|r| r.command.changes()).count(), 0, "{at}: nothing changes in Slack");
    }
}

#[test]
fn several_files_are_picked_from_a_list() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let files = vec![
            file("F1", "one.pdf", "PDF", Some(1), true),
            file("F2", "elsewhere", "Google Docs", None, false),
            file("F3", "two.png", "PNG", Some(2), true),
            file("F4", "three.txt", "Plain Text", Some(3), true),
        ];
        let mut s = shown(files, w, h, lang, Some(scratch("several")));
        s.keys("f");
        assert!(asked(&s).is_none(), "{at}: nothing asked before a pick");
        let screen = s.d.screen();
        let title = s.d.app.i18n.label(Label::FileListTitle).to_string();
        assert!(screen.contains(&title), "{at}:\n{screen}");
        assert!(
            !screen.lines().any(|l| l.contains(" a ") && l.contains("elsewhere")),
            "{at}: only files that download"
        );
        // A letter picks.
        s.keys("s");
        assert_eq!(asked(&s).map(|(_, f)| f.id), Some(FileId::new("F3")), "{at}");
        assert!(s.d.app.file_picker().is_none(), "{at}");
        // j and Enter pick; Esc closes, nothing asked.
        s.b.held.clear();
        s.keys("f j j enter");
        assert_eq!(asked(&s).map(|(_, f)| f.id), Some(FileId::new("F4")), "{at}");
        s.b.held.clear();
        s.keys("f esc");
        assert!(asked(&s).is_none() && s.d.app.file_picker().is_none(), "{at}");
        // A click on a row picks it.
        s.keys("f");
        let screen = s.d.screen();
        let y = screen.lines().position(|l| l.contains("one.pdf")).unwrap() as u16;
        let x = screen.lines().nth(y as usize).unwrap().find("one.pdf").unwrap() as u16;
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        s.pump();
        assert_eq!(asked(&s).map(|(_, f)| f.id), Some(FileId::new("F1")), "{at}");
    }
}

#[test]
fn a_kind_a_viewer_does_not_show_is_only_saved() {
    let mut s = shown(vec![file("F1", "deploy.sh", "Shell", Some(10), true)], 120, 40, Lang::En, Some(scratch("sh")));
    s.keys("f");
    let (id, f) = asked(&s).unwrap();
    let path = PathBuf::from("/downloads/deploy.sh");
    s.b.held.clear();
    s.b.push(Envelope { generation: id, event: saved(f.id, &path, false) });
    s.pump();
    assert_eq!(effects(&mut s), []);
    assert_eq!(s.d.app.question(), None, "a script is never offered to open");
    let said =
        s.d.app.i18n.msg(&Msg::FileSavedOnly { name: "deploy.sh".into(), folder: "/downloads".into() }).to_string();
    assert!(s.d.status_line().contains(&said), "{}", s.d.status_line());
}

#[test]
fn nothing_to_download_says_so_and_asks_nothing() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let i18n = I18n::new(lang);
        let mut s = shown(Vec::new(), w, h, lang, Some(scratch("none")));
        s.keys("f");
        assert!(s.d.status_line().contains(&*i18n.label(Label::FileNone)), "{at}: {}", s.d.status_line());
        let mut s = shown(vec![file("F2", "elsewhere", "Google Docs", None, false)], w, h, lang, Some(scratch("x")));
        s.keys("f");
        let said = i18n.label(Label::FileNotDownloadable).to_string();
        assert!(s.d.status_line().contains(&said), "{at}: {}", s.d.status_line());
        let mut s = shown(vec![file("F1", "a.pdf", "PDF", Some(1), true)], w, h, lang, None);
        s.keys("f");
        let said = i18n.label(Label::FileNoFolder).to_string();
        assert!(s.d.status_line().contains(&said), "{at}: {}", s.d.status_line());
        assert!(asked(&s).is_none(), "{at}");
    }
}

#[test]
fn a_failed_download_says_why() {
    for (kind, label) in [
        (FailKind::Unsaved, Label::FailUnsaved),
        (FailKind::Api { code: "redirected_elsewhere".into() }, Label::FileWhyElsewhere),
        (FailKind::Api { code: "not_a_file".into() }, Label::FileWhyPage),
        (FailKind::Api { code: "file_not_available".into() }, Label::FileWhyGone),
        (FailKind::Offline, Label::FailOffline),
        (FailKind::Stalled, Label::FailStalled),
    ] {
        let mut s = shown(vec![file("F1", "a.pdf", "PDF", Some(1), true)], 120, 40, Lang::En, Some(scratch("fail")));
        s.keys("f");
        let i = s.b.held.iter().position(|r| matches!(r.command, Command::Download { .. })).unwrap();
        s.b.fail(i, kind.clone());
        s.pump();
        let why = s.d.app.i18n.label(label).to_string();
        let said = s.d.app.i18n.msg(&Msg::FileFailed { why }).to_string();
        assert!(s.d.status_line().contains(&said), "{kind:?}: {}", s.d.status_line());
        assert!(effects(&mut s).iter().all(|e| !matches!(e, Effect::OpenFile(_))));
    }
}

/// The demo's channel with files, opened, and the newest message with one selected.
fn demo_with_files(lang: Lang, w: u16, h: u16, dir: &std::path::Path) -> Demo {
    let mut d = Demo::with(w, h, lang, slakio_tui::app::Settings::default());
    d.app.downloads.dir = Some(dir.to_path_buf());
    d.app.downloads.max = 1024 * 1024 * 1024;
    d.open("design-review");
    let world = slakio_world::World::demo();
    let c = world.snapshot().conversations.iter().find(|c| c.name == "design-review").unwrap().id.clone();
    let n = world.message_count(&c);
    let up =
        (0..n).rev().position(|i| world.message(&c, i).unwrap().files.iter().any(|f| f.download.is_some())).unwrap();
    d.keys("G");
    for _ in 0..up {
        d.keys("k");
    }
    d
}

#[test]
fn the_demo_saves_a_file_in_the_folder() {
    for (lang, w, h) in SIZES {
        let dir = scratch(&format!("demo-{lang:?}-{w}"));
        let mut d = demo_with_files(lang, w, h, &dir);
        assert_harmless(&d.screen());
        d.keys("f");
        if d.app.file_picker().is_some() {
            d.keys("a");
        }
        let saved: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(saved.len(), 1, "{saved:?}");
        assert!(!saved[0].starts_with('.'), "no part file left: {saved:?}");
        // Invented bytes: never offered to open, whatever their kind.
        assert_eq!(d.app.question(), None, "{lang:?} {w}x{h}");
        assert!(d.status_line().contains(&saved[0]), "{}", d.status_line());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn hostile_file_names_draw_harmlessly() {
    for (lang, w, h) in SIZES {
        let mut d = Demo::with(w, h, lang, slakio_tui::app::Settings::default());
        d.open("hostile-strings");
        d.keys("g g");
        let screen = d.screen();
        assert_harmless(&screen);
        assert!(screen.contains("../../.ssh/authoriz"), "{lang:?} {w}x{h}:\n{screen}");
        assert!(!screen.contains("\u{202E}"));
    }
}

#[test]
fn snapshots_of_the_cards_and_the_list() {
    for (lang, w, h) in [(Lang::En, 120, 40), (Lang::En, 80, 24)] {
        let files = vec![
            file("F1", "Q3-report.pdf", "PDF", Some(1_234_567), true),
            file("F2", "login-screen.png", "PNG", Some(345_678), true),
            file("F3", "Shared plan", "Google Docs", None, false),
        ];
        let mut s = shown(files, w, h, lang, Some(scratch("snap")));
        insta::assert_snapshot!(format!("files_cards_{w}x{h}"), s.d.snap());
        s.keys("f");
        insta::assert_snapshot!(format!("files_list_{w}x{h}"), s.d.snap());
    }
}

#[test]
fn a_file_that_could_not_be_marked_is_kept_but_never_offered_to_open() {
    let mut s = shown(vec![file("F1", "a.pdf", "PDF", Some(10), true)], 120, 40, Lang::En, Some(scratch("unmarked")));
    s.keys("f");
    let (id, f) = asked(&s).unwrap();
    s.b.held.clear();
    s.b.push(Envelope { generation: id, event: saved(f.id, &PathBuf::from("/downloads/a.pdf"), true) });
    s.pump();
    assert_eq!(s.d.app.question(), None, "not marked: not opened from here");
    let said =
        s.d.app.i18n.msg(&Msg::FileSavedUnmarked { name: "a.pdf".into(), folder: "/downloads".into() }).to_string();
    assert!(s.d.status_line().contains(&said), "{}", s.d.status_line());
    assert!(effects(&mut s).iter().all(|e| !matches!(e, Effect::OpenFile(_))));
}

#[test]
fn a_file_on_its_way_is_not_asked_for_again_and_two_are_said_as_two() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let files = vec![file("F1", "one.pdf", "PDF", Some(100), true), file("F3", "two.png", "PNG", Some(200), true)];
        let mut s = shown(files, w, h, lang, Some(scratch("twice")));
        s.keys("f a");
        s.keys("f a");
        let downloads =
            |s: &Scripted| s.b.held.iter().filter(|r| matches!(r.command, Command::Download { .. })).count();
        assert_eq!(downloads(&s), 1, "{at}: the same file is not asked for twice");
        let again = s.d.app.i18n.msg(&Msg::FileAlready { name: "one.pdf".into() }).to_string();
        assert!(s.d.status_line().contains(&again), "{at}: {}", s.d.status_line());
        s.keys("f s");
        assert_eq!(downloads(&s), 2, "{at}");
        let two = s.d.app.i18n.msg(&Msg::FileDownloadingMany { count: 2, stop: "F".into() }).to_string();
        assert!(s.d.status_line().contains(&two), "{at}: {}", s.d.status_line());
        // Progress of either one keeps saying two: the line does not jump between them.
        let (id, f) = asked(&s).unwrap();
        s.b.push(Envelope { generation: id, event: Event::Progress { file: f.id, got: 100, total: Some(200) } });
        s.pump();
        assert!(s.d.status_line().contains(&two), "{at}: {}", s.d.status_line());
    }
}

#[test]
fn f_shift_stops_the_downloads_on_their_way_and_so_does_the_command() {
    for how in ["F", ":download cancel"] {
        let mut s = shown(vec![file("F1", "a.pdf", "PDF", Some(10), true)], 120, 40, Lang::En, Some(scratch("stop")));
        let i18n = I18n::new(Lang::En);
        let stop = |s: &mut Scripted| {
            if how == "F" {
                s.keys("F");
            } else {
                s.keys(":");
                s.d.type_text(&how[1..]);
                s.keys("enter");
            }
        };
        stop(&mut s);
        let none = i18n.label(Label::FileNoDownload).to_string();
        assert!(s.d.status_line().contains(&none), "{how}: {}", s.d.status_line());
        s.keys("f");
        let (id, _) = asked(&s).unwrap();
        stop(&mut s);
        let cancel = s.b.held.iter().find(|r| r.command == Command::Cancel { request: id });
        let cancel = cancel.unwrap_or_else(|| panic!("{how}: a cancel of the download"));
        assert!(cancel.workspace.is_some(), "{how}: to the download's own workspace");
        let stopping = i18n.label(Label::FileCancelling).to_string();
        assert!(s.d.status_line().contains(&stopping), "{how}: {}", s.d.status_line());
        let i = s.b.held.iter().position(|r| r.id == id).unwrap();
        s.b.fail(i, FailKind::Cancelled);
        s.pump();
        let said = i18n.msg(&Msg::FileCancelled { name: "a.pdf".into() }).to_string();
        assert!(s.d.status_line().contains(&said), "{how}: {}", s.d.status_line());
        assert!(effects(&mut s).iter().all(|e| !matches!(e, Effect::OpenFile(_))));
    }
}

#[test]
fn copying_a_message_keeps_the_names_of_its_files() {
    let files = vec![file("F1", "a.pdf", "PDF", Some(10), true), file("F2", "plan", "Google Docs", None, false)];
    let mut s = shown_with("", files.clone(), 120, 40, Lang::En, None);
    s.keys("y");
    let copied: Vec<Effect> = effects(&mut s).into_iter().filter(|e| matches!(e, Effect::Copy(_))).collect();
    assert_eq!(copied, [Effect::Copy("\u{1F4CE} a.pdf\n\u{1F4CE} plan".into())], "a file-only message");
    let mut s = shown_with("see", files, 120, 40, Lang::En, None);
    s.keys("y");
    let copied: Vec<Effect> = effects(&mut s).into_iter().filter(|e| matches!(e, Effect::Copy(_))).collect();
    assert_eq!(copied, [Effect::Copy("see\n\u{1F4CE} a.pdf\n\u{1F4CE} plan".into())]);
}

/// The download held, answered `event` now.
fn answer(s: &mut Scripted, id: slakio_core::backend::Generation, event: Event) {
    s.b.held.retain(|r| r.id != id);
    s.b.push(Envelope { generation: id, event });
    s.pump();
}

#[test]
fn a_download_stays_said_until_it_ends_however_long_it_waits() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let i18n = I18n::new(lang);
        let mut s =
            shown(vec![file("F1", "roadmap.xlsx", "Excel", Some(98_000), true)], w, h, lang, Some(scratch("long")));
        s.keys("f");
        let (id, f) = asked(&s).unwrap();
        let waiting = i18n.msg(&Msg::FileWaiting { name: "roadmap.xlsx".into(), stop: "F".into() }).to_string();
        // No byte for 9 s: still waited for, the stop key still shown.
        s.d.now += std::time::Duration::from_secs(9);
        assert!(s.d.status_line().contains(&waiting), "{at}: {}", s.d.status_line());
        // 50 KB of 98 KB, then nothing for a minute: how much stays said.
        let progress = Event::Progress { file: f.id.clone(), got: 50_000, total: Some(98_000) };
        s.b.push(Envelope { generation: id, event: progress });
        s.pump();
        let half = Msg::FileProgress { name: "roadmap.xlsx".into(), percent: "51".into(), stop: "F".into() };
        let half = i18n.msg(&half).to_string();
        s.d.now += std::time::Duration::from_secs(60);
        assert!(s.d.status_line().contains(&half), "{at}: {}", s.d.status_line());
        // Another notice covers it for its time; then it is back.
        s.keys("F");
        s.d.app.downloads.dir = None;
        s.keys("f");
        s.d.now += std::time::Duration::from_secs(10);
        let stopping = i18n.label(Label::FileCancelling).to_string();
        assert!(s.d.status_line().contains(&stopping), "{at}: {}", s.d.status_line());
        // Ended: said once, then gone.
        s.b.fail(s.b.held.iter().position(|r| r.id == id).unwrap(), FailKind::Cancelled);
        s.pump();
        let stopped = i18n.msg(&Msg::FileCancelled { name: "roadmap.xlsx".into() }).to_string();
        assert!(s.d.status_line().contains(&stopped), "{at}: {}", s.d.status_line());
        s.d.now += std::time::Duration::from_secs(10);
        let line = s.d.status_line();
        assert!(!line.contains(&stopped) && !line.contains(&stopping), "{at}: {line}");
    }
}

#[test]
fn a_file_saved_after_a_failure_is_said_in_its_place_with_its_folder() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let i18n = I18n::new(lang);
        let home = scratch("home");
        let dir = home.join("Downloads").join("slakio");
        let files = vec![
            file("F1", "full-backup.tar", "Tar", Some(54_000_000_000), true),
            file("F2", "a.pdf", "PDF", Some(10), true),
        ];
        let mut s = shown(files, w, h, lang, Some(dir.clone()));
        s.d.app.downloads.home = Some(home);
        s.d.app.downloads.max = 1_024_000_000;
        s.keys("f a");
        let i = s.b.held.iter().position(|r| matches!(r.command, Command::Download { .. })).unwrap();
        s.b.fail(i, FailKind::TooLarge);
        s.pump();
        // The limit is said.
        let why = i18n.msg(&Msg::FileWhyTooLarge { limit: "1024".into() }).to_string();
        let failed = i18n.msg(&Msg::FileFailed { why }).to_string();
        assert!(s.d.status_line().contains(&failed), "{at}: {}", s.d.status_line());
        s.keys("f s");
        let (id, f) = asked(&s).unwrap();
        answer(&mut s, id, saved(f.id, &dir.join("a.pdf"), false));
        let folder = format!("~{}Downloads{}slakio", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR);
        let said = i18n.msg(&Msg::FileSaved { name: "a.pdf".into(), folder }).to_string();
        let line = s.d.status_line();
        assert!(line.contains(&said) && !line.contains(&failed), "{at}: {line}");
    }
}

#[test]
fn downloads_stopped_at_once_are_counted() {
    let files = vec![file("F1", "one.pdf", "PDF", Some(100), true), file("F3", "two.png", "PNG", Some(200), true)];
    let mut s = shown(files, 120, 40, Lang::En, Some(scratch("stop-two")));
    s.keys("f a f s F");
    for _ in 0..2 {
        let i = s.b.held.iter().position(|r| matches!(r.command, Command::Download { .. })).unwrap();
        s.b.fail(i, FailKind::Cancelled);
        s.pump();
    }
    let said = s.d.app.i18n.msg(&Msg::FileCancelledMany { count: 2 }).to_string();
    assert!(s.d.status_line().contains(&said), "{}", s.d.status_line());
}

#[test]
fn a_cut_file_name_keeps_its_extension_and_the_size_is_never_cut() {
    let long = "2026\u{B144} 3\u{BD84}\u{AE30} \u{B514}\u{C790}\u{C778} \u{B9AC}\u{BDF0} \u{CD5C}\u{C885}\u{BCF8} (\u{C218}\u{C815}\u{BCF8}) \u{BC30}\u{D3EC} \u{C804} \u{AC80}\u{D1A0}\u{C6A9} \u{D68C}\u{C758}\u{B85D}.pdf";
    for (lang, w, h) in [(Lang::En, 80, 24), (Lang::Ko, 80, 24), (Lang::En, 50, 24)] {
        let at = format!("{lang:?} {w}x{h}");
        let files = vec![
            file("F1", long, "PDF", Some(812), true),
            file("F2", "roadmap.xlsx", "Excel Spreadsheet", Some(98_000), true),
            file("F3", "\u{BC1C}\u{D45C}\u{C790}\u{B8CC}.pptx", "Microsoft PowerPoint Presentation", Some(812), true),
        ];
        let dir = scratch("long-name");
        let mut s = shown(files, w, h, lang, Some(dir.clone()));
        let screen = s.d.screen();
        let cards: Vec<&str> = screen.lines().filter(|l| l.contains('\u{21E9}')).collect();
        let card = cards.first().unwrap_or_else(|| panic!("{at}:\n{screen}"));
        assert!(card.contains("2026") && card.contains(".pdf") && card.contains('…'), "{at}: {card}");
        // Tight: the name before the size.
        let card = cards.get(1).unwrap_or_else(|| panic!("{at}:\n{screen}"));
        assert!(card.contains("roadmap.xlsx") || card.contains("98 KB"), "{at}: {card}");
        if w == 50 {
            assert!(card.contains("roadmap.xlsx"), "{at}: {card}");
        }
        // The list: the size whole on each row, the kind dropped first.
        s.keys("f");
        let screen = s.d.screen();
        for (name, size) in [(".pdf", "812 B"), ("roadmap.xlsx", "98 KB"), (".pptx", "812 B")] {
            let row = screen.lines().find(|l| l.contains(name)).unwrap_or_else(|| panic!("{at}:\n{screen}"));
            assert!(row.contains(size), "{at}: {row}");
        }
        // The question names the file with its extension.
        s.keys("a");
        let (id, f) = asked(&s).unwrap();
        answer(&mut s, id, saved(f.id, &dir.join(long), false));
        assert_eq!(s.d.app.question(), Some(Question::File), "{at}");
        let screen = s.d.screen();
        assert!(screen.contains("\u{D68C}\u{C758}\u{B85D}.pdf"), "{at}:\n{screen}");
        assert!(!screen.contains("…. "), "{at}:\n{screen}");
    }
}

#[test]
fn a_narrow_status_line_keeps_how_much_and_the_stop_key_over_the_name() {
    for lang in [Lang::En, Lang::Ko] {
        let i18n = I18n::new(lang);
        let files = vec![file("F1", "roadmap-for-the-next-quarter.xlsx", "Excel", Some(98_000), true)];
        let mut s = shown(files, 50, 24, lang, Some(scratch("narrow")));
        s.keys("f");
        let (id, f) = asked(&s).unwrap();
        s.b.push(Envelope { generation: id, event: Event::Progress { file: f.id, got: 40_000, total: Some(98_000) } });
        s.pump();
        let short = i18n.msg(&Msg::FileProgressShort { percent: "40".into(), stop: "F".into() }).to_string();
        assert!(s.d.status_line().contains(&short), "{lang:?}: {}", s.d.status_line());
    }
}
