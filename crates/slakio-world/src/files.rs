//! Files shared in the world's messages: some messages of `design-review` share one to three
//! files (documents, pictures, a script, one too large to download, one hosted elsewhere), and
//! the first message of `hostile-strings` shares files whose names attack a file system or a
//! terminal (paths that climb out of a folder, a reversed extension, escape sequences, a
//! reserved Windows name). Each file's content is invented from its id ([`content`]), so the demo
//! and the tests' fake Slack serve the same bytes.

use crate::rng::Rng;
use slakio_core::model::{FILE_HOST, File, FileId};

/// The channel whose messages share files.
pub const FILES_CHANNEL: &str = "design-review";

/// (name, kind, size) of the files `design-review` shares.
const SAMPLES: &[(&str, &str, u64)] = &[
    ("Q3-report.pdf", "PDF", 1_234_567),
    ("login-screen.png", "PNG", 345_678),
    ("notes.txt", "Plain Text", 2_048),
    ("deploy.sh", "Shell", 812),
    ("roadmap.xlsx", "Excel Spreadsheet", 98_304),
    ("demo-recording.mp4", "MPEG 4 Video", 48_000_000),
    ("Install Helper.app.zip", "Zip", 5_120_000),
];

/// A file too large for a download's default cap.
const HUGE: (&str, &str, u64) = ("full-backup.tar", "Tarball", 53_687_091_200);

/// (name, kind, size) of the files the hostile channel shares.
const HOSTILE: &[(&str, &str, u64)] = &[
    ("../../.ssh/authorized_keys", "Plain Text", 400),
    ("invoice_\u{202E}fdp.exe", "Binary", 70_000),
    ("evil\x1b]0;owned\x07\x1b[2J.txt", "Plain Text", 64),
    ("CON.txt", "Plain Text", 10),
    ("..", "Binary", 1),
];

/// The address a file is downloaded from.
fn address(team: &str, id: &FileId, name: &str) -> String {
    let path: String = name
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => char::from(b).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("https://{FILE_HOST}/files-pri/{team}-{id}/download/{path}")
}

fn file(team: &str, id: String, (name, kind, size): (&str, &str, u64), download: bool) -> File {
    let id = FileId::new(id);
    let download = download.then(|| address(team, &id, name));
    let thumb = download.as_ref().and_then(|_| crate::pictures::thumb(team, id.as_str(), name));
    File { id, name: name.into(), kind: kind.into(), size: Some(size), download, thumb }
}

/// The files message `index` of `conversation` (named `name`, of workspace `team`) shares.
pub fn of(seed: u64, team: &str, conversation: &str, name: &str, index: usize) -> Vec<File> {
    let id = |k: usize| format!("F{conversation}{index:05}{k}");
    match name {
        crate::names::HOSTILE if index == 0 => {
            HOSTILE.iter().enumerate().map(|(k, s)| file(team, id(k), *s, true)).collect()
        }
        FILES_CHANNEL => {
            let mut r = Rng::keyed(seed, &format!("files/{conversation}"), index as u64);
            if !r.chance(25) {
                return Vec::new();
            }
            (0..1 + r.below(3) as usize)
                .map(|k| match r.below(12) {
                    0 => file(team, id(k), HUGE, true),
                    // Hosted elsewhere (a linked document): nothing to download.
                    1 => file(team, id(k), ("Shared plan", "Google Docs", 0), false),
                    _ => file(team, id(k), *r.pick(SAMPLES), true),
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// The bytes of file `id`, `size` of them (at most `max`): the same every time.
pub fn content(id: &FileId, size: u64, max: u64) -> Vec<u8> {
    let line = format!("slakio demo file {id}\n");
    line.bytes().cycle().take(usize::try_from(size.min(max)).unwrap_or(0)).collect()
}
