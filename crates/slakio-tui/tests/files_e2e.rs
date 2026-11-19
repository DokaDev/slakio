//! Downloading a file with the real binary in a pseudo terminal, against the tests' fake Slack,
//! at 120x40 and 80x24, in English and Korean: a Slack link to a message of `design-review`
//! that shares files selects it, `f` (and a letter when it shares several) downloads one: one
//! request to the file host's path with the session's cookie and not the token, the file saved
//! in the downloads folder (`SLAKIO_DOWNLOAD_DIR`) with the fake's bytes, marked as from the
//! internet as soon as it has its name (on macOS: its quarantine attribute, read with
//! `xattr -p`), and nothing opened unless asked. Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use slakio_core::i18n::{I18n, Lang, Msg};
use slakio_fake::{FILES, FakeSlack};
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
    fn new(name: &str, lang: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-files-e2e-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        let settings = format!("icons = \"off\"\ncache = \"off\"\nlive = \"off\"\nlanguage = \"{lang}\"\n");
        std::fs::write(dir.join("config.toml"), settings).unwrap();
        Self { _rt: rt, fake, world, dir }
    }

    fn start(&self, size: (u16, u16)) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let env = [("SLAKIO_TEST_CREDENTIALS", creds.as_str()), ("SLAKIO_TEST_SLACK_ADDR", addr.as_str())];
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, size)
    }

    fn downloads(&self) -> Vec<String> {
        std::fs::read_dir(self.dir.join("downloads"))
            .map(|d| d.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What notice `msg` says up to the file's `name`, its folder (`\u{1}`, cut to the room) left out.
fn around(i18n: &I18n, msg: Msg, name: &str) -> String {
    let said = i18n.msg(&msg).to_string();
    let piece = said.split('\u{1}').find(|p| p.contains(name)).unwrap_or_default();
    piece[..piece.find(name).unwrap_or_default() + name.len()].to_string()
}

fn has(t: &str, w: &str) -> bool {
    t.replace(' ', "").contains(&w.replace(' ', ""))
}

#[test]
fn a_file_is_downloaded_into_the_folder_from_the_file_host_only() {
    for (size, lang) in [((120, 40), Lang::En), ((80, 24), Lang::En), ((120, 40), Lang::Ko), ((80, 24), Lang::Ko)] {
        let code = if lang == Lang::Ko { "ko" } else { "en" };
        let m = Machine::new(&format!("{}-{code}", size.0), code);
        // The newest message of design-review with a small file that downloads.
        let ws = m.world.snapshot().workspaces[0].clone();
        let c = m.world.find(ws.id.as_str(), "design-review").unwrap().id.clone();
        let n = m.world.message_count(&c);
        let (msg, file) = (0..n)
            .rev()
            .filter_map(|i| m.world.message(&c, i))
            .find_map(|msg| {
                let f = msg.files.iter().find(|f| f.download.is_some() && f.size.is_some_and(|s| s < 100_000))?.clone();
                Some((msg, f))
            })
            .expect("a small file");
        let Some(mut run) = m.start(size) else { return };
        let i18n = I18n::new(lang);
        run.wait_for("the list", |t| t.contains("deploys"));
        wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
        std::thread::sleep(Duration::from_millis(300));
        let link = format!("https://example.slack.com/archives/{c}/p{:016}", msg.ts.0);
        run.type_bytes(format!(":open {link}\r").as_bytes());
        run.wait_for("the message", |t| t.contains("design-review") && has(t, file.name.unsanitized()));
        let asked = m.fake.log().len();
        run.type_bytes(b"f");
        let downloadable = msg.files.iter().filter(|f| f.download.is_some()).count();
        if downloadable > 1 {
            let title = i18n.label(slakio_core::i18n::Label::FileListTitle).to_string();
            run.wait_for("the list of files", |t| has(t, &title));
            let k = msg.files.iter().filter(|f| f.download.is_some()).position(|f| f.id == file.id).unwrap();
            let letter = ["a", "s", "d"][k];
            run.type_bytes(letter.as_bytes());
        }
        wait_until("the file saved", || m.downloads().iter().any(|n| n == file.name.unsanitized()));
        let saved = m.dir.join("downloads").join(file.name.unsanitized());
        assert_eq!(
            std::fs::read(&saved).unwrap(),
            slakio_world::files::content(&file.id, file.size.unwrap(), u64::MAX)
        );
        let name = file.name.unsanitized().to_string();
        let said = around(&i18n, Msg::FileSaved { name: name.clone(), folder: "\u{1}".into() }, &name);
        let only = around(&i18n, Msg::FileSavedOnly { name: name.clone(), folder: "\u{1}".into() }, &name);
        run.wait_for("saved, said", |t| has(t, &said) || has(t, &only));
        // Only the file host's path was asked, once, with the cookie and without the token.
        let log = m.fake.log()[asked..].to_vec();
        let files: Vec<_> = log.iter().filter(|l| l.method() == FILES).collect();
        assert_eq!(files.len(), 1, "{size:?} {code}: {:?}", log.iter().map(|l| l.method()).collect::<Vec<_>>());
        assert!(log.iter().all(|l| l.method() == FILES || l.method() == "users/info"), "{size:?} {code}");
        let cookie = files[0].headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("cookie")).map(|(_, v)| v.clone());
        assert!(cookie.is_some_and(|c| c.contains(COOKIE)));
        assert!(files[0].headers.iter().all(|(_, v)| !v.contains(TOKEN)) && !files[0].query.contains(TOKEN));
        // Marked as from the internet (macOS: before it had its name, so it is marked now).
        if cfg!(target_os = "macos") {
            let mut read = std::process::Command::new("/usr/bin/xattr");
            let marked = read.args(["-p", "com.apple.quarantine"]).arg(&saved).output().unwrap();
            let value = String::from_utf8_lossy(&marked.stdout).into_owned();
            assert!(marked.status.success() && value.contains(";slakio;"), "{size:?} {code}: {value:?}");
        }
        // Asked to open: not now.
        if slakio_core::download::openable(&saved) {
            let title = i18n.label(slakio_core::i18n::Label::DialogFileTitle).to_string();
            run.wait_for("the question", |t| has(t, &title));
            run.type_bytes(b"n");
            run.wait_for("the question gone", |t| !has(t, &title));
        }
        run.quit();
    }
}
