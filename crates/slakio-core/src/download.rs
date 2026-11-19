//! Saving a downloaded file: the name it is saved under, where, and the bytes written as they
//! come, so a backend never chooses a path from what a remote party wrote.
//!
//! * The name ([`safe_name`]) is the remote name sanitised and made a plain file name: only
//!   what follows its last `/` or `\`, no character a file system refuses or reads as a path,
//!   no leading dot or dash (no hidden file, no `..`, nothing read as an option), no trailing
//!   dot or space, no reserved Windows name, at most [`NAME_MAX`] bytes with its extension kept.
//! * The bytes go to a hidden part file in the download folder, made new (never through what is
//!   there) and readable by the user alone, at most `max` of them ([`Saving::write`]); a part
//!   file not finished is removed, and one a run that is gone left behind is swept ([`sweep`]).
//!   A download folder slakio makes is the user's alone too.
//! * Finished, it is marked as downloaded from the internet while it is still the part file
//!   (macOS: the quarantine attribute on the open file; Windows: the `Zone.Identifier` stream),
//!   so it never shows under its name unmarked; then it takes the first free name, `name`,
//!   `name (2).ext`, `name (3).ext` …, renamed without replacing anything
//!   ([`crate::fsutil::rename_new`]): nothing is ever written over.
//!
//! Which saved files the app offers to open is an allowlist of kinds that open in a viewer
//! ([`openable`]); anything else (a program, a script, an installer, a document that can carry
//! code or formulas, an unknown kind) is only saved.

use crate::sanitize::Remote;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The most bytes of a saved file's name.
pub const NAME_MAX: usize = 120;

/// The most names tried for a file before giving up (`name (999).ext`).
const TRIES: u32 = 999;

/// The name a file Slack calls `name` is saved under.
pub fn safe_name(name: &Remote) -> String {
    // The base name and its extension are sanitised apart, so a long name keeps its extension
    // (the sanitiser cuts a long line).
    let raw = name.unsanitized();
    let base = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let (stem, ext) = split(base);
    let ext = if ext.chars().count() <= 16 { Remote::new(ext).line().into_string() } else { String::new() };
    let line = format!("{}{ext}", Remote::new(stem).line().as_str());
    let mut out: String = line
        .chars()
        .map(|c| match c {
            '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\u{FFFD}' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out.trim_start_matches(['.', ' ', '-']).trim_end_matches(['.', ' ']).to_string();
    out = shorten(&out, NAME_MAX);
    // Cutting may leave a trailing dot or space again.
    out = out.trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        out = "file".to_string();
    }
    if reserved(&out) {
        out.insert(0, '_');
    }
    out
}

/// `name` at most `max` bytes, its extension kept when it is short.
fn shorten(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = split(name);
    let ext = if ext.len() <= 16 { ext } else { "" };
    let room = max - ext.len();
    let mut cut = room.min(stem.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{ext}", &stem[..cut])
}

/// `name` as its stem and its extension (with its dot; empty when it has none).
fn split(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// A name Windows keeps for a device, with any extension (`CON`, `nul.txt`, `CONIN$`, `COM1`,
/// `LPT²`: the superscript digits count as digits there).
fn reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_uppercase();
    let port = |n: &str| {
        let mut c = n.chars();
        matches!((c.next(), c.next()), (Some('1'..='9' | '\u{B9}' | '\u{B2}' | '\u{B3}'), None))
    };
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT")).is_some_and(port)
}

/// The `n`th name tried for `name` (from 1): `name` itself, then `stem (n).ext`.
pub fn nth_name(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_string();
    }
    let (stem, ext) = split(name);
    format!("{stem} ({n}){ext}")
}

/// Why a file was not saved.
#[derive(Debug)]
pub enum Unsaved {
    /// It is larger than the most allowed.
    TooLarge,
    /// The folder or the file could not be written.
    Io(io::Error),
}

impl From<io::Error> for Unsaved {
    fn from(e: io::Error) -> Self {
        Unsaved::Io(e)
    }
}

/// A file being saved: its bytes go to a part file, renamed into place when finished; one
/// dropped before that is removed.
#[derive(Debug)]
pub struct Saving {
    dir: PathBuf,
    name: String,
    part: PathBuf,
    file: Option<fs::File>,
    written: u64,
    max: u64,
}

/// A file saved ([`Saving::finish`]): where it is, and whether the system's mark of a file
/// from the internet could not be set on it (it is kept all the same; it should not be opened
/// from here then).
#[derive(Debug)]
pub struct Finished {
    pub path: PathBuf,
    pub unmarked: bool,
}

impl Saving {
    /// Start saving the file Slack calls `name` in `dir`, at most `max` bytes. A folder that is
    /// not there is made, the user's alone ([`crate::fsutil::private_dir`]); one that is there is
    /// left as it is.
    pub fn start(dir: &Path, name: &Remote, max: u64) -> Result<Self, Unsaved> {
        if !dir.is_dir() {
            crate::fsutil::private_dir(dir)?;
        }
        let name = safe_name(name);
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut n = 0u32;
        let (part, file) = loop {
            n += 1;
            let part = dir.join(format!(".{name}.{}-{n}.part", std::process::id()));
            match opts.open(&part) {
                Ok(f) => break (part, f),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < TRIES => {}
                Err(e) => return Err(e.into()),
            }
        };
        Ok(Self { dir: dir.to_path_buf(), name, part, file: Some(file), written: 0, max })
    }

    /// The name the file is saved under, when nothing else has it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Bytes written so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// Write the next `bytes`: refused (nothing written) past the most allowed.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), Unsaved> {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if self.written.saturating_add(len) > self.max {
            return Err(Unsaved::TooLarge);
        }
        let file = self.file.as_mut().ok_or_else(|| io::Error::other("already finished"))?;
        file.write_all(bytes)?;
        self.written += len;
        Ok(())
    }

    /// Every byte is written: mark it as from the internet ([`mark`]), flush it and give it the
    /// first free name.
    pub fn finish(mut self) -> Result<Finished, Unsaved> {
        let file = self.file.take().ok_or_else(|| io::Error::other("already finished"))?;
        let unmarked = !mark(&file, &self.part);
        file.sync_all()?;
        drop(file);
        for n in 1..=TRIES {
            let to = self.dir.join(nth_name(&self.name, n));
            match crate::fsutil::rename_new(&self.part, &to) {
                Ok(()) => return Ok(Finished { path: to, unmarked }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(io::Error::from(io::ErrorKind::AlreadyExists).into())
    }
}

impl Drop for Saving {
    fn drop(&mut self) {
        // Not finished (finishing moves the part file away, so nothing is there then).
        drop(self.file.take());
        let _ = fs::remove_file(&self.part);
    }
}

/// The kinds of file the app offers to open once saved, by extension: PDF, pictures, sound,
/// video and plain text a viewer shows. Programs, scripts, installers, disk images, archives,
/// links, web pages, office documents (they may carry macros, embedded objects or external
/// references: `.docx`, `.xlsx`, `.pptx`, `.odt`, `.pages` …), tables a spreadsheet reads
/// formulas from (`.csv`, `.tsv`), `.md` and `.json` (often opened by an editor that runs
/// code) and anything not listed are only saved.
const OPENABLE: &[&str] = &[
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "heic", "bmp", "tif", "tiff", "txt", "log", "mp3", "m4a", "wav", "aac",
    "flac", "mp4", "mov", "m4v", "webm",
];

/// A file saved as `path` is of a kind the app offers to open ([`OPENABLE`]).
pub fn openable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| OPENABLE.contains(&e.to_ascii_lowercase().as_str()))
}

/// The mark of a file downloaded from the internet, set on the part file `file` (at `part`)
/// before it gets its name, so the system checks it as it checks a browser's downloads: macOS's
/// quarantine attribute, set on the open file (never through a path a link could redirect).
/// `false` when it could not be set.
#[cfg(target_os = "macos")]
fn mark(file: &fs::File, _part: &Path) -> bool {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let value = format!("0081;{secs:08x};slakio;");
    rustix::fs::fsetxattr(file, QUARANTINE, value.as_bytes(), rustix::fs::XattrFlags::empty()).is_ok()
}

/// The attribute macOS reads a downloaded file's origin from.
#[cfg(target_os = "macos")]
pub const QUARANTINE: &str = "com.apple.quarantine";

/// Windows' mark: the part file's `Zone.Identifier` stream says it came from the internet
/// zone, as a browser's download says.
#[cfg(windows)]
fn mark(_file: &fs::File, part: &Path) -> bool {
    let mut stream = part.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    fs::write(stream, ZONE_IDENTIFIER).is_ok()
}

/// What a downloaded file's `Zone.Identifier` stream says (Windows).
#[cfg(windows)]
pub const ZONE_IDENTIFIER: &str = "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=about:internet\r\n";

/// Elsewhere the system keeps no such mark: nothing to set.
#[cfg(not(any(target_os = "macos", windows)))]
fn mark(_file: &fs::File, _part: &Path) -> bool {
    true
}

/// A part file not written for this long is left over, whoever wrote it (a download that
/// stalls fails long before).
const STALE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Remove the part files that runs of slakio which are gone left in `dir` (killed, crashed, the
/// power lost): `.<name>.<pid>-<n>.part`, whose process no longer runs (macOS, Linux) or that
/// was not written for a day. Nothing else is touched; a folder that is not there is fine.
pub fn sweep(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(part_pid) else { continue };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        let old = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age > STALE);
        if meta.is_file() && (old || !running(pid)) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// The process id in the name of a part file ([`Saving::start`]): `.<name>.<pid>-<n>.part`.
fn part_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix('.')?.strip_suffix(".part")?;
    let (pid, n) = rest.rsplit_once('.')?.1.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (digits(pid) && digits(n)).then(|| pid.parse().ok()).flatten()
}

/// Process `pid` runs (this one does; one of another user counts as running).
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    let Some(p) = i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw) else { return false };
    !matches!(rustix::process::test_kill_process(p), Err(rustix::io::Errno::SRCH))
}

/// Not known here: only the part file's age tells.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn running(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests;
