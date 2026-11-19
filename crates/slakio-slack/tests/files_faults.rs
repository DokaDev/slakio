//! A download that goes wrong on its way, against the fake Slack: an answer cut short of its
//! length, a server error, a request to slow down, the network switched off while the bytes
//! come, a download stalled, stopped by the user, a third one at once, a folder that cannot be
//! written, and the app quitting in the middle. Each ends with one answer and nothing left in the
//! folder: no file, no part file.

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::model::File;
use slakio_core::secret::Credentials;
use slakio_fake::{FILES, FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const MAX: u64 = 64 * 1024 * 1024;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-faults-{}-{tag}", std::process::id()));
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
            Credentials::new(concat!("xo", "xc-TEST"), "cookie-TEST"),
            Endpoints::local(fake.addr()).unwrap(),
            Arc::clone(&governor),
        );
        Self { fake, handle: slakio_slack::serve(client), governor, world: World::demo(), id: 0 }
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

    /// Send `command`; its id.
    fn send(&mut self, command: Command) -> Generation {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        Generation(self.id)
    }

    /// Ask for `file` into `dir`; the download's id.
    fn start(&mut self, file: File, dir: &Path) -> Generation {
        self.send(Command::Download { file, dir: dir.to_path_buf(), max: MAX })
    }

    /// The next event.
    async fn next(&mut self) -> Envelope {
        tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("an event").unwrap()
    }

    /// Events until download `id`'s answer: that answer.
    async fn answer(&mut self, id: Generation) -> Event {
        loop {
            let e = self.next().await;
            if e.generation == id && !matches!(e.event, Event::Progress { .. }) {
                return e.event;
            }
        }
    }

    /// Wait until download `id` told how it is going, and its part file is in `dir`.
    async fn receiving(&mut self, id: Generation, dir: &Path) {
        loop {
            let e = self.next().await;
            assert_eq!(e.generation, id, "{e:?}");
            if matches!(e.event, Event::Progress { .. }) {
                break;
            }
        }
        for _ in 0..500 {
            if names(dir).iter().any(|n| n.ends_with(".part")) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no part file in {dir:?}");
    }

    /// The next file request is answered with `length` bytes said and a few sent, then nothing.
    fn stall_next(&self) {
        self.fake.fail_next(FILES, Fault::Partial { length: 100_000, body: vec![b'x'; 1000] });
    }
}

fn failed(event: &Event) -> (FailKind, String) {
    match event {
        Event::Failed { kind, detail, .. } => (kind.clone(), detail.clone()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_answer_cut_short_of_its_length_is_not_saved() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("short");
        let said = vec![("content-length".to_string(), "10".to_string())];
        r.fake.fail_next(FILES, Fault::Raw { status: 200, headers: said, body: b"abc".to_vec() });
        let id = r.start(r.small_file(), &dir);
        assert_eq!(failed(&r.answer(id).await).0, FailKind::Network);
        assert!(names(&dir).is_empty(), "{:?}", names(&dir));
    });
}

#[test]
fn a_server_error_is_a_network_failure_and_slow_down_says_how_long() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("status");
        r.fake.fail_next(FILES, Fault::Status { status: 503, retry_after: None });
        let id = r.start(r.small_file(), &dir);
        assert_eq!(failed(&r.answer(id).await).0, FailKind::Network);
        let wait = vec![("retry-after".to_string(), "7".to_string())];
        r.fake.fail_next(FILES, Fault::Raw { status: 429, headers: wait, body: Vec::new() });
        let id = r.start(r.small_file(), &dir);
        let limited = FailKind::RateLimited { retry_after: Some(Duration::from_secs(7)) };
        assert_eq!(failed(&r.answer(id).await).0, limited);
        assert!(names(&dir).is_empty(), "{:?}", names(&dir));
    });
}

#[test]
fn switching_the_network_off_while_the_bytes_come_keeps_nothing() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("off");
        r.stall_next();
        let id = r.start(r.small_file(), &dir);
        r.receiving(id, &dir).await;
        r.governor.stop();
        assert_eq!(failed(&r.answer(id).await).0, FailKind::Offline);
        assert!(names(&dir).is_empty(), "{:?}", names(&dir));
    });
}

#[test]
fn a_stalled_download_ends_after_its_idle_time_and_keeps_nothing() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("stall");
        r.stall_next();
        let id = r.start(r.small_file(), &dir);
        r.receiving(id, &dir).await;
        // The 30 s pass at once (the clock is moved by hand: the writer's thread keeps it from
        // running ahead by itself).
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(31)).await;
        let (kind, detail) = failed(&r.answer(id).await);
        assert_eq!((kind, detail.as_str()), (FailKind::Network, "the download stalled"));
        assert!(names(&dir).is_empty(), "{:?}", names(&dir));
    });
}

#[test]
fn a_download_stopped_by_the_user_keeps_nothing_and_a_third_at_once_is_busy() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("cancel");
        r.stall_next();
        r.stall_next();
        let first = r.start(r.small_file(), &dir);
        r.receiving(first, &dir).await;
        let second = r.start(r.small_file(), &dir);
        let third = r.start(r.small_file(), &dir);
        // The third is answered at once; the second's progress may come before it.
        let mut busy = None;
        while busy.is_none() {
            let e = r.next().await;
            if e.generation == third {
                busy = Some(failed(&e.event).0);
            }
        }
        assert_eq!(busy, Some(FailKind::Busy));
        for id in [first, second] {
            let cancel = r.send(Command::Cancel { request: id });
            let (mut stopped, mut told) = (None, None);
            while stopped.is_none() || told.is_none() {
                let e = r.next().await;
                match e.event {
                    Event::Cancelled { request, stopping } if e.generation == cancel => {
                        told = Some((request, stopping))
                    }
                    Event::Progress { .. } => {}
                    event if e.generation == id => stopped = Some(failed(&event).0),
                    other => panic!("{other:?}"),
                }
            }
            assert_eq!(told, Some((id, true)));
            assert_eq!(stopped, Some(FailKind::Cancelled));
        }
        assert!(names(&dir).is_empty(), "{:?}", names(&dir));
        // Nothing on its way: said so, and nothing else comes.
        let cancel = r.send(Command::Cancel { request: first });
        let e = r.next().await;
        assert_eq!((e.generation, e.event), (cancel, Event::Cancelled { request: first, stopping: false }));
    });
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_written_saves_nothing() {
    use std::os::unix::fs::PermissionsExt;
    rt().block_on(async {
        let mut r = Run::new().await;
        let dir = temp_dir("locked");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let id = r.start(r.small_file(), &dir);
        assert_eq!(failed(&r.answer(id).await).0, FailKind::Unsaved);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(names(&dir).is_empty(), "{:?}", names(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn quitting_in_the_middle_of_a_download_leaves_no_part_file() {
    let dir = temp_dir("quit");
    let rt = rt();
    let r = rt.block_on(async {
        let mut r = Run::new().await;
        r.stall_next();
        let id = r.start(r.small_file(), &dir);
        r.receiving(id, &dir).await;
        r
    });
    // The app's end: its runtime goes, and every task on it.
    drop(rt);
    drop(r);
    assert!(names(&dir).is_empty(), "{:?}", names(&dir));
}
