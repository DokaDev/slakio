//! Saving a downloaded file: the name it is saved under, where, and the bytes written as they
//! come, so a backend never chooses a path from what a remote party wrote.
//!
//! * The name ([`safe_name`]) is the remote name sanitised and made a plain file name: only
//!   what follows its last `/` or `\`, no character a file system refuses or reads as a path,
//!   no leading dot or dash (no hidden file, no `..`, nothing read as an option), no trailing
//!   dot or space, no reserved Windows name, at most [`NAME_MAX`] bytes with its extension kept.
//! * The bytes go to a hidden part file in the download folder, made new (never through what is
//!   there), at most `max` of them ([`Saving::write`]); a part file not finished is removed.
//! * Finished, it takes the first free name, `name`, `name (2).ext`, `name (3).ext` …, renamed
//!   without replacing anything ([`crate::fsutil::rename_new`]): nothing is ever written over.
//!
//! Which saved files the app offers to open is an allowlist of kinds that open in a viewer
//! ([`openable`]); anything else (a program, a script, an installer, an unknown kind) is only
//! saved.

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

/// A name Windows keeps for a device, with any extension (`CON`, `nul.txt`, `COM1`).
fn reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
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

impl Saving {
    /// Start saving the file Slack calls `name` in `dir` (made when missing), at most `max`
    /// bytes.
    pub fn start(dir: &Path, name: &Remote, max: u64) -> Result<Self, Unsaved> {
        fs::create_dir_all(dir)?;
        let name = safe_name(name);
        let mut n = 0u32;
        let (part, file) = loop {
            n += 1;
            let part = dir.join(format!(".{name}.{}-{n}.part", std::process::id()));
            match fs::OpenOptions::new().write(true).create_new(true).open(&part) {
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

    /// Every byte is written: flush it and give it the first free name. The path it has.
    pub fn finish(mut self) -> Result<PathBuf, Unsaved> {
        let file = self.file.take().ok_or_else(|| io::Error::other("already finished"))?;
        file.sync_all()?;
        drop(file);
        for n in 1..=TRIES {
            let to = self.dir.join(nth_name(&self.name, n));
            match crate::fsutil::rename_new(&self.part, &to) {
                Ok(()) => return Ok(to),
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

/// The kinds of file the app offers to open once saved, by extension: documents, pictures,
/// sound, video and plain text a viewer shows. Programs, scripts, installers, disk images,
/// archives, links, web pages, documents that may carry macros or embedded objects (`.doc`,
/// `.xls`, `.ppt`, `.rtf`, `.docm` …) and anything not listed are only saved.
const OPENABLE: &[&str] = &[
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "heic", "bmp", "tif", "tiff", "txt", "md", "csv", "tsv", "log", "json",
    "docx", "xlsx", "pptx", "pages", "numbers", "key", "odt", "ods", "odp", "mp3", "m4a", "wav", "aac", "flac", "mp4",
    "mov", "m4v", "webm",
];

/// A file saved as `path` is of a kind the app offers to open ([`OPENABLE`]).
pub fn openable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| OPENABLE.contains(&e.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests;
