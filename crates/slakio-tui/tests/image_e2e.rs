//! Pictures with the real binary in a pseudo terminal, against the tests' fake Slack, with
//! pictures asked for (`SLAKIO_IMAGES=on`, `avatars = "image"`): a message of `design-review`
//! sharing a picture opened by its link, its smaller copy fetched from the file host's path with
//! the session's cookie, the senders' photos from the avatar host's path with no cookie and no
//! token, the pictures sent to the terminal (kitty graphics, never asking for an answer) and
//! drawn as placeholder cells, and every picture removed from the terminal on the way out. And
//! the default in a terminal that is neither kitty nor Ghostty: nothing fetched, nothing sent.
//! Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use slakio_fake::{FakeSlack, PICTURES};
use slakio_world::World;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Machine {
    _rt: tokio::runtime::Runtime,
    fake: FakeSlack,
    world: World,
    dir: PathBuf,
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Machine {
    fn new(name: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-image-e2e-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        let settings = "icons = \"off\"\ncache = \"off\"\nlive = \"off\"\nlanguage = \"en\"\navatars = \"image\"\n";
        std::fs::write(dir.join("config.toml"), settings).unwrap();
        Self { _rt: rt, fake, world, dir }
    }

    fn start(&self, env: &[(&str, &str)]) -> Option<Run> {
        self.start_at(env, (120, 40))
    }

    /// [`Machine::start`] in a terminal of `size` (columns, rows).
    fn start_at(&self, env: &[(&str, &str)], size: (u16, u16)) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let mut all = vec![("SLAKIO_TEST_CREDENTIALS", creds.as_str()), ("SLAKIO_TEST_SLACK_ADDR", addr.as_str())];
        all.extend_from_slice(env);
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &all, size)
    }

    fn written(&self) -> Vec<u8> {
        std::fs::read(self.dir.join("typescript")).unwrap_or_default()
    }

    /// The link of the newest message of `design-review` that shares a picture.
    fn picture_link(&self) -> String {
        let ws = self.world.snapshot().workspaces[0].clone();
        let c = self.world.find(ws.id.as_str(), "design-review").unwrap().id.clone();
        let n = self.world.message_count(&c);
        let msg = (0..n)
            .rev()
            .filter_map(|i| self.world.message(&c, i))
            .find(|m| m.files.iter().any(|f| f.thumb.is_some()))
            .expect("a message sharing a picture");
        format!("https://example.slack.com/archives/{c}/p{:016}", msg.ts.0)
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn pictures_come_from_their_hosts_are_drawn_and_removed_on_the_way_out() {
    let m = Machine::new("on");
    let Some(mut run) = m.start(&[("SLAKIO_IMAGES", "on"), ("TERM", "xterm-kitty")]) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
    std::thread::sleep(Duration::from_millis(300));
    run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
    run.wait_for("the message", |t| t.contains("design-review"));
    let pictures = || m.fake.log().into_iter().filter(|l| l.method() == PICTURES).collect::<Vec<_>>();
    wait_until("a smaller copy fetched", || pictures().iter().any(|l| l.path.starts_with("/files-tmb/")));
    wait_until("a photo fetched", || pictures().iter().any(|l| slakio_world::pictures::is_avatar_path(&l.path)));
    wait_until("a picture sent", || contains(&m.written(), b"\x1b_Ga=T,U=1,f=100"));
    wait_until("placeholders drawn", || contains(&m.written(), "\u{10EEEE}".as_bytes()));
    for l in pictures() {
        let cookie = l.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("cookie")).map(|(_, v)| v.clone());
        if slakio_world::pictures::is_avatar_path(&l.path) {
            assert_eq!(cookie, None, "{}: a photo goes with no cookie", l.path);
        } else {
            assert!(cookie.is_some_and(|c| c.contains(COOKIE)), "{}", l.path);
        }
        assert!(l.headers.iter().all(|(_, v)| !v.contains(TOKEN)) && !l.query.contains(TOKEN), "never the token");
    }
    let written = m.written();
    // Every graphics command asks for no answer.
    let mut at = 0;
    while let Some(i) = written[at..].windows(3).position(|w| w == b"\x1b_G") {
        let start = at + i;
        let end = written[start..].windows(2).position(|w| w == b"\x1b\\").map_or(written.len(), |e| start + e);
        let command = &written[start..end];
        let keys = command.split(|b| *b == b';').next().unwrap();
        assert!(contains(keys, b"q=2"), "{}", String::from_utf8_lossy(&keys[..keys.len().min(80)]));
        at = end;
    }
    run.quit();
    let written = m.written();
    assert!(contains(&written, b"\x1b_Ga=d,d=R,x=1,y="), "every id sent removed with its data on the way out");
    assert!(contains(&written, b"\x1b_Ga=d,d=A,q=2\x1b\\"), "every picture removed on the way out");
}

#[test]
fn a_terminal_that_is_not_kitty_or_ghostty_gets_text_only() {
    let m = Machine::new("off");
    let Some(mut run) = m.start(&[("SLAKIO_IMAGES", "auto"), ("TERM", "xterm-256color")]) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
    std::thread::sleep(Duration::from_millis(300));
    run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
    run.wait_for("the message", |t| t.contains("design-review"));
    std::thread::sleep(Duration::from_millis(800));
    assert!(m.fake.log().iter().all(|l| l.method() != PICTURES), "nothing fetched");
    run.quit();
    assert!(!contains(&m.written(), b"\x1b_G"), "nothing sent");
    assert!(!contains(&m.written(), "\u{10EEEE}".as_bytes()));
}

#[test]
fn images_off_from_the_palette_clears_them_and_is_kept_for_the_next_run() {
    let m = Machine::new("palette");
    // Kitty, the variable empty: the setting decides (`auto`, the default).
    let env = [("SLAKIO_IMAGES", ""), ("TERM", "xterm-kitty")];
    let Some(mut run) = m.start(&env) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
    std::thread::sleep(Duration::from_millis(300));
    run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
    run.wait_for("the message", |t| t.contains("design-review"));
    wait_until("a picture sent", || contains(&m.written(), b"\x1b_Ga=T,U=1,f=100"));
    run.type_bytes(b":images off\r");
    wait_until("every picture removed", || contains(&m.written(), b"\x1b_Ga=d,d=A,q=2\x1b\\"));
    run.wait_for("said", |t| t.contains("Images: off"));
    let at = m.written().len();
    // Moving about draws again: nothing more is sent.
    run.type_bytes(b"kkjj");
    std::thread::sleep(Duration::from_millis(800));
    let after = m.written()[at..].to_vec();
    assert!(!contains(&after, b"\x1b_Ga=T") && !contains(&after, "\u{10EEEE}".as_bytes()), "nothing after off");
    let fetched = m.fake.log().iter().filter(|l| l.method() == PICTURES).count();
    run.quit();
    let config = std::fs::read_to_string(m.dir.join("config.toml")).unwrap();
    assert!(config.contains("images = \"off\""), "{config}");
    // The next run keeps it: nothing fetched, nothing sent.
    let _ = std::fs::remove_file(m.dir.join("typescript"));
    let Some(mut run) = m.start(&env) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
    run.wait_for("the message", |t| t.contains("design-review"));
    std::thread::sleep(Duration::from_millis(800));
    run.quit();
    assert_eq!(m.fake.log().iter().filter(|l| l.method() == PICTURES).count(), fetched, "nothing fetched");
    assert!(!contains(&m.written(), b"\x1b_Ga=T"));
}

#[test]
fn a_decoder_panic_leaves_the_screen_as_it_is_and_the_app_goes_on() {
    let m = Machine::new("panic");
    let env = [("SLAKIO_IMAGES", "on"), ("TERM", "xterm-kitty"), ("SLAKIO_DEBUG_DECODE_PANIC", "1")];
    let Some(mut run) = m.start(&env) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
    std::thread::sleep(Duration::from_millis(300));
    run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
    run.wait_for("the message", |t| t.contains("design-review"));
    wait_until("a picture fetched", || m.fake.log().iter().any(|l| l.method() == PICTURES));
    run.wait_for("the card says there is no preview", |t| t.contains("no preview"));
    let written = m.written();
    assert!(!contains(&written, b"\x1b[?1049l"), "the alternate screen was not left");
    assert!(!contains(&written, b"panicked at"), "no panic message on the screen");
    assert!(!contains(&written, b"\x1b_Ga=T"), "nothing sent for it");
    // Keys still work: the help opens and closes.
    run.type_bytes(b"?");
    run.wait_for("the help", |t| t.contains("Keys —"));
    run.type_bytes(b"\x1b");
    run.wait_for("the help closed", |t| !t.contains("Keys —"));
    run.quit();
    let log = std::fs::read_to_string(m.dir.join("state").join("slakio").join("errors.log")).unwrap_or_default();
    assert!(log.contains("the picture decoder panicked"), "{log}");
    assert!(!log.contains("https://") && !log.contains("files-tmb") && !log.contains("avatars/"), "{log}");
}

#[test]
fn a_panic_while_a_picture_is_kept_leaves_it_shown_and_says_so() {
    let m = Machine::new("keep-panic");
    let env = [("SLAKIO_IMAGES", "on"), ("TERM", "xterm-kitty"), ("SLAKIO_DEBUG_DECODE_PANIC", "keep")];
    let Some(mut run) = m.start(&env) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
    std::thread::sleep(Duration::from_millis(300));
    run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
    run.wait_for("the message", |t| t.contains("design-review"));
    wait_until("the picture sent", || contains(&m.written(), b"\x1b_Ga=T,U=1,f=100"));
    let errors = m.dir.join("state").join("slakio").join("errors.log");
    let log = || std::fs::read_to_string(&errors).unwrap_or_default();
    wait_until("the panic logged", || log().contains("keeping a picture on disk panicked"));
    run.quit();
    let log = log();
    assert!(!log.contains("the picture decoder panicked"), "shown, so not the decoder: {log}");
    assert!(!log.contains("https://") && !log.contains("files-tmb") && !log.contains("avatars/"), "{log}");
}

/// A terminal tall enough for every message the opened conversation shows, its pictures and
/// files' cards drawn, with rows to spare.
const TALL: (u16, u16) = (120, 300);

#[test]
fn a_second_run_draws_the_pictures_kept_on_disk_without_fetching_them() {
    let m = Machine::new("kept");
    let config = std::fs::read_to_string(m.dir.join("config.toml")).unwrap();
    std::fs::write(m.dir.join("config.toml"), config.replace("cache = \"off\"", "cache = \"on\"")).unwrap();
    // A stand-in for tmutil that records what it is asked to exclude from backups.
    let tmutil = m.dir.join("tmutil");
    let calls = m.dir.join("tmutil.log");
    std::fs::write(&tmutil, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n", calls.display())).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&tmutil, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let tmutil = tmutil.display().to_string();
    let env = [("SLAKIO_IMAGES", "on"), ("TERM", "xterm-kitty"), ("SLAKIO_TEST_TMUTIL", tmutil.as_str())];
    // A file's preview is never kept on disk: fetched each run.
    let fetches = || m.fake.log().into_iter().filter(|l| l.method() == PICTURES).collect::<Vec<_>>();
    let previews = || fetches().iter().filter(|l| l.path.starts_with("/files-tmb/")).count();
    let pictures = || fetches().len() - previews();
    let kept = m.dir.join("cache").join("slakio").join("pictures");
    let files = || std::fs::read_dir(&kept).map_or(0, |d| d.count());
    // Until nothing more is fetched (the DM rows' photos come four at a time, a slow machine
    // takes a while between them): two seconds with no new fetch, previews too, and (`kept`)
    // every picture fetched kept.
    let settle = |what: &str, kept: bool| {
        let settled = std::cell::Cell::new((usize::MAX, Instant::now()));
        wait_until(what, || {
            let now = fetches().len();
            if (kept && files() != pictures()) || now != settled.get().0 {
                settled.set((now, Instant::now()));
            }
            settled.get().1.elapsed() >= Duration::from_secs(2)
        });
    };
    let Some(mut run) = m.start_at(&env, TALL) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
    std::thread::sleep(Duration::from_millis(300));
    // The list shows every DM row (the first to the last), so each peer's photo is one on screen
    // and none is left to ask for ahead in a later run.
    run.type_bytes(b" d");
    run.wait_for("every DM row", |t| t.contains("Alex Morgan") && t.contains("Mallory"));
    run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
    run.wait_for("the message", |t| t.contains("design-review"));
    wait_until("a picture sent", || contains(&m.written(), b"\x1b_Ga=T,U=1,f=100"));
    // Everything fetched is kept before the run ends.
    wait_until("each picture kept", || files() > 0 && files() == pictures());
    settle("the pictures settled", true);
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(files(), pictures());
    // The messages fit with room to spare, the preview drawn: nothing was pushed off the screen
    // while it came (a photo shown only before it came would be one run's and not the other's).
    let screen = run.output();
    let lines: Vec<Vec<char>> = screen.lines().map(|l| l.chars().collect()).collect();
    let title = lines.iter().position(|l| l.iter().collect::<String>().contains("▌#design-review")).expect("its title");
    let left = lines[title].iter().rposition(|c| *c == '╭').expect("the timeline's corner");
    let top: String = lines[title + 1].iter().skip(left + 1).take_while(|c| **c != '│').collect();
    assert!(top.trim().is_empty(), "the timeline's top row is free: {top:?}\n{screen}");
    run.quit();
    let (fetched, previewed) = (pictures(), previews());
    assert!(fetched >= 1 && previewed >= 1, "a preview and photos: {fetched} {previewed}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&kept).unwrap().permissions().mode() & 0o777, 0o700);
        for f in std::fs::read_dir(&kept).unwrap().flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            assert!(name.len() == 36 && name.ends_with(".png"), "{name}");
            assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600, "{name}");
        }
    }
    let _ = std::fs::remove_file(m.dir.join("typescript"));
    let Some(mut run) = m.start_at(&env, TALL) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    run.type_bytes(b" d");
    run.wait_for("every DM row", |t| t.contains("Alex Morgan") && t.contains("Mallory"));
    run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
    run.wait_for("the message", |t| t.contains("design-review"));
    wait_until("a picture sent", || contains(&m.written(), b"\x1b_Ga=T,U=1,f=100"));
    wait_until("placeholders drawn", || contains(&m.written(), "\u{10EEEE}".as_bytes()));
    // As long as the first run: whatever it would fetch again, it has by then.
    settle("the second run's pictures settled", false);
    std::thread::sleep(Duration::from_millis(800));
    run.quit();
    assert_eq!(pictures(), fetched, "nothing fetched again: every photo came from the disk");
    assert!(previews() > previewed, "the preview fetched again: never kept");
    // Kept out of backups when it was made, once.
    let excluded = std::fs::read_to_string(&calls).unwrap_or_default();
    let line = format!("addexclusion\n{}\n", kept.display());
    assert_eq!(excluded.matches(&line).count(), 1, "the pictures excluded once: {excluded}");
}

#[test]
fn a_picture_on_disk_that_makes_the_decoder_panic_is_removed() {
    let m = Machine::new("kept-panic");
    let config = std::fs::read_to_string(m.dir.join("config.toml")).unwrap();
    std::fs::write(m.dir.join("config.toml"), config.replace("cache = \"off\"", "cache = \"on\"")).unwrap();
    let kept = m.dir.join("cache").join("slakio").join("pictures");
    // The pictures kept (not a write's temporary file, which a run that ends may leave).
    let files = || {
        let names = std::fs::read_dir(&kept).into_iter().flatten().flatten();
        names.filter(|e| !e.file_name().to_string_lossy().starts_with('.')).count()
    };
    // Both runs go the same way, so the second looks for what the first kept.
    let go = |env: &[(&str, &str)]| {
        let mut run = m.start(env)?;
        run.wait_for("the list", |t| t.contains("deploys"));
        wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
        std::thread::sleep(Duration::from_millis(300));
        run.type_bytes(format!(":open {}\r", m.picture_link()).as_bytes());
        run.wait_for("the message", |t| t.contains("design-review"));
        Some(run)
    };
    let Some(run) = go(&[("SLAKIO_IMAGES", "on"), ("TERM", "xterm-kitty")]) else { return };
    wait_until("a photo kept", || files() > 0);
    // Until nothing more is kept.
    let mut before = 0;
    while files() != before {
        before = files();
        std::thread::sleep(Duration::from_millis(800));
    }
    run.quit();
    // The next run's decoder panics on every picture, those read from the disk too.
    let _ = std::fs::remove_file(m.dir.join("typescript"));
    let env = [("SLAKIO_IMAGES", "on"), ("TERM", "xterm-kitty"), ("SLAKIO_DEBUG_DECODE_PANIC", "1")];
    let Some(run) = go(&env) else { return };
    wait_until("each picture that set it off removed", || files() == 0);
    run.quit();
}
