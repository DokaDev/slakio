//! Downloading the files messages share, against the fake Slack: the bytes saved under a safe
//! name in the folder asked for, never over a file there; the session's cookie sent to the file
//! host and the token never; redirects followed only on the file host; a file too large, a web
//! page instead of a file, an address elsewhere and a switched-off network refused without
//! anything saved (and, where it can be known first, without anything asked).

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::model::{FILE_HOST, File, FileId};
use slakio_core::secret::Credentials;
use slakio_fake::{FILES, FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");
const MAX: u64 = 64 * 1024 * 1024;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-files-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

struct Run {
    fake: FakeSlack,
    handle: BackendHandle,
    governor: Arc<Governor>,
    world: World,
    id: u64,
}

impl Run {
    async fn new() -> Self {
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(
            Credentials::new(TOKEN, "cookie-TEST"),
            Endpoints::local(fake.addr()).unwrap(),
            Arc::clone(&governor),
        );
        Self { fake, handle: slakio_slack::serve(client), governor, world: World::demo(), id: 0 }
    }

    /// The files of message `index` of the first workspace's conversation `name`.
    fn files(&self, name: &str, index: usize) -> Vec<File> {
        let s = self.world.snapshot();
        let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
        self.world.message(&c.id, index).unwrap().files
    }

    /// A file of `design-review` that can be downloaded and is small.
    fn small_file(&self) -> File {
        let s = self.world.snapshot();
        let c = s.conversations.iter().find(|c| c.name == "design-review").unwrap();
        (0..self.world.message_count(&c.id))
            .flat_map(|i| self.world.message(&c.id, i).unwrap().files)
            .find(|f| f.download.is_some() && f.size.is_some_and(|s| s < 100_000))
            .expect("a small file")
    }

    /// Download `file` into `dir`: every event until its answer (the answer last).
    async fn download(&mut self, file: File, dir: &Path, max: u64) -> Vec<Event> {
        self.id += 1;
        let command = Command::Download { file, dir: dir.to_path_buf(), max };
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let mut events = Vec::new();
        loop {
            let e: Envelope =
                tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("an answer").unwrap();
            assert_eq!(e.generation, Generation(self.id));
            let end = matches!(e.event, Event::Downloaded { .. } | Event::Failed { .. });
            events.push(e.event);
            if end {
                return events;
            }
        }
    }

    fn file_requests(&self) -> Vec<slakio_fake::Logged> {
        self.fake.log().into_iter().filter(|l| l.method() == FILES).collect()
    }
}

fn failed(events: &[Event]) -> FailKind {
    match events.last() {
        Some(Event::Failed { kind, .. }) => kind.clone(),
        other => panic!("{other:?}"),
    }
}

fn api(code: &str) -> FailKind {
    FailKind::Api { code: code.to_string() }
}

#[test]
fn a_file_is_saved_with_its_bytes_and_only_the_cookie_goes_to_the_file_host() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("saved");
        let file = r.small_file();
        let events = r.download(file.clone(), &dir, MAX).await;
        let Some(Event::Downloaded { file: id, path, bytes }) = events.last() else { panic!("{events:?}") };
        assert_eq!(id, &file.id);
        assert_eq!(path.parent(), Some(dir.as_path()));
        assert_eq!(path.file_name().unwrap().to_string_lossy(), file.name.unsanitized());
        let want = slakio_world::files::content(&file.id, file.size.unwrap(), u64::MAX);
        assert_eq!(std::fs::read(path).unwrap(), want);
        assert_eq!(*bytes, want.len() as u64);
        assert!(events.iter().any(|e| matches!(e, Event::Progress { .. })), "progress is told");
        let asked = r.file_requests();
        assert_eq!(asked.len(), 1);
        let l = &asked[0];
        assert!(l.path.starts_with("/files-pri/"), "{}", l.path);
        let header = |n: &str| l.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.as_str());
        assert_eq!(header("cookie"), Some("d=cookie-TEST"));
        assert_eq!(header("referer"), Some("https://app.slack.com/"));
        assert_eq!(header("origin"), None);
        assert!(header("authorization").is_none());
        assert!(!l.query.contains(TOKEN) && !l.path.contains(TOKEN) && l.body.is_empty(), "the token is never sent");
        assert!(l.headers.iter().all(|(_, v)| !v.contains(TOKEN)));
        // Again: a new name, the first file kept.
        let events = r.download(file.clone(), &dir, MAX).await;
        let Some(Event::Downloaded { path, .. }) = events.last() else { panic!("{events:?}") };
        assert!(path.file_name().unwrap().to_string_lossy().contains(" (2)"), "{path:?}");
        assert_eq!(names(&dir).len(), 2, "nothing written over, no part file left");
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn a_hostile_name_is_saved_inside_the_folder() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("hostile");
        let files = r.files("hostile-strings", 0);
        assert!(files.len() >= 4);
        let mut saved = Vec::new();
        for f in files {
            let events = r.download(f, &dir, MAX).await;
            let Some(Event::Downloaded { path, .. }) = events.last() else { panic!("{events:?}") };
            assert_eq!(path.parent(), Some(dir.as_path()), "{path:?}");
            saved.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
        assert_eq!(saved[0], "authorized_keys");
        assert!(
            saved.iter().all(|n| !n.starts_with('.') && !n.contains('\u{202E}') && !n.contains('\x1b')),
            "{saved:?}"
        );
        assert!(saved.contains(&"_CON.txt".to_string()), "{saved:?}");
        assert!(saved.contains(&"file".to_string()), "{saved:?}");
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn a_file_said_to_be_too_large_is_not_asked_for() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("huge");
        let mut file = r.small_file();
        file.size = Some(MAX + 1);
        assert_eq!(failed(&r.download(file, &dir, MAX).await), FailKind::TooLarge);
        assert!(r.file_requests().is_empty(), "nothing was asked");
        assert!(names(&dir).is_empty());
    });
}

#[test]
fn an_answer_longer_than_the_cap_is_not_saved() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("long");
        let mut file = r.small_file();
        file.size = None;
        // Said too long.
        let said = vec![("content-length".to_string(), "999999999999".to_string())];
        r.fake.fail_next(FILES, Fault::Raw { status: 200, headers: said, body: b"x".to_vec() });
        assert_eq!(failed(&r.download(file.clone(), &dir, 1000).await), FailKind::TooLarge);
        // Not said, but longer.
        let unsaid = vec![("transfer-encoding".to_string(), "chunked".to_string())];
        let mut body = b"7d0\r\n".to_vec();
        body.extend(std::iter::repeat_n(b'y', 2000));
        body.extend_from_slice(b"\r\n0\r\n\r\n");
        r.fake.fail_next(FILES, Fault::Raw { status: 200, headers: unsaid, body });
        assert_eq!(failed(&r.download(file, &dir, 1000).await), FailKind::TooLarge);
        assert!(names(&dir).is_empty(), "no part file is left: {:?}", names(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn a_web_page_instead_of_a_file_is_not_saved() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("page");
        let page = vec![("content-type".to_string(), "text/html; charset=utf-8".to_string())];
        r.fake.fail_next(FILES, Fault::Raw { status: 200, headers: page, body: b"<html>sign in</html>".to_vec() });
        assert_eq!(failed(&r.download(r.small_file(), &dir, MAX).await), api("not_a_file"));
        assert!(names(&dir).is_empty());
    });
}

#[test]
fn a_redirect_is_followed_on_the_file_host_only() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("redirect");
        let file = r.small_file();
        let to =
            |l: &str| Fault::Raw { status: 302, headers: vec![("location".to_string(), l.to_string())], body: vec![] };
        // Elsewhere: refused, nothing more asked.
        r.fake.fail_next(FILES, to("https://evil.example/steal"));
        assert_eq!(failed(&r.download(file.clone(), &dir, MAX).await), api("redirected_elsewhere"));
        assert_eq!(r.file_requests().len(), 1);
        let plain = format!("http://{FILE_HOST}/x");
        let longer = format!("https://{FILE_HOST}.evil.example/x");
        for elsewhere in ["//evil.example/x", plain.as_str(), longer.as_str()] {
            r.fake.fail_next(FILES, to(elsewhere));
            assert_eq!(failed(&r.download(file.clone(), &dir, MAX).await), api("redirected_elsewhere"), "{elsewhere}");
        }
        // On the file host: followed, the cookie still sent there.
        let path = file.download.as_deref().unwrap().strip_prefix(&format!("https://{FILE_HOST}")).unwrap().to_string();
        r.fake.fail_next(FILES, to(&path));
        let events = r.download(file.clone(), &dir, MAX).await;
        assert!(matches!(events.last(), Some(Event::Downloaded { .. })), "{events:?}");
        let last = r.file_requests().pop().unwrap();
        assert!(last.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("cookie") && v == "d=cookie-TEST"));
        // Too many.
        for _ in 0..4 {
            r.fake.fail_next(FILES, to(&path));
        }
        assert_eq!(failed(&r.download(file, &dir, MAX).await), api("too_many_redirects"));
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn an_address_elsewhere_or_none_is_not_asked() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("elsewhere");
        let bad = [
            "https://evil.example/files-pri/x".to_string(),
            format!("http://{FILE_HOST}/x"),
            format!("https://{FILE_HOST}@evil.test/x"),
        ];
        for url in bad.iter().map(|u| Some(u.as_str())).chain([None]) {
            let file = File {
                id: FileId::new("FX"),
                name: "x.txt".into(),
                kind: "Plain Text".into(),
                size: Some(1),
                download: url.map(str::to_string),
            };
            assert_eq!(failed(&r.download(file, &dir, MAX).await), api("file_not_downloadable"), "{url:?}");
        }
        assert!(r.file_requests().is_empty());
    });
}

#[test]
fn a_missing_file_and_a_switched_off_network_save_nothing() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("missing");
        let mut file = r.small_file();
        file.download = Some(format!("https://{FILE_HOST}/files-pri/T-{}/download/x", "FNOPE000000"));
        assert_eq!(failed(&r.download(file, &dir, MAX).await), api("file_not_available"));
        r.governor.stop();
        let asked = r.file_requests().len();
        assert_eq!(failed(&r.download(r.small_file(), &dir, MAX).await), FailKind::Offline);
        assert_eq!(r.file_requests().len(), asked, "nothing was sent");
        assert!(names(&dir).is_empty());
    });
}
