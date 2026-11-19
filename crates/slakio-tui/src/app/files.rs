//! The files a message shares: downloaded (`f`, `:download`), saved, then opened when asked.
//!
//! `f` on a message with one file that can be downloaded asks the backend for it at once; with
//! several, a list shows them (their letters as the link picker's, its keys the `links`
//! context's: `j`/`k`, `Enter`, `Esc`; a click on a row, the wheel), and the one picked is asked
//! for. A file hosted elsewhere, or none, is said. Downloading is a read: nothing changes in
//! Slack, so a read-only workspace downloads too.
//!
//! While a file comes, the status line says how much has; once saved, a file of a kind a viewer
//! shows ([`slakio_core::download::openable`]) is offered to open (`dialog::Question::File`, the
//! safe answer focused: nothing opens by itself); any other kind is only saved, and the status
//! line says so. Every file saved is marked as downloaded from the internet (macOS quarantine,
//! [`Effect::Quarantine`]) before anything may open it.

use super::dialog::{Dialog, Question};
use super::picker::{LETTERS, hint_letters, list_rect};
use super::status::Level;
use super::{App, Effect};
use crate::action::{Action, LinksAction};
use crate::keymap::{Ctx, KeyChord};
use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use slakio_core::backend::{Command, FailKind, Generation};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::{File, FileId, WorkspaceId};
use std::path::PathBuf;
use std::time::Instant;

/// Where files are saved and how large one may be (set by the binary from the config).
#[derive(Clone, Debug, Default)]
pub struct Downloads {
    /// The folder; `None` when there is none (no home directory, nothing configured).
    pub dir: Option<PathBuf>,
    /// The most bytes of a file.
    pub max: u64,
    /// The downloads asked for and not answered yet.
    going: Vec<Going>,
    /// The file saved that the question offers to open.
    saved: Option<PathBuf>,
}

/// A download asked for: its request, its file and the name said for it.
#[derive(Clone, Debug)]
struct Going {
    id: Generation,
    file: FileId,
    name: String,
}

/// The files of a message to pick one of, shown as a list.
#[derive(Clone, Debug)]
pub struct FilePicker {
    workspace: WorkspaceId,
    pub files: Vec<File>,
    /// The letters of each file.
    pub hints: Vec<String>,
    /// The letters typed so far.
    pub typed: String,
    pub cursor: usize,
}

/// `bytes` in words (`812 B`, `345 KB`, `1.2 MB`), as a file manager says them (1000 a unit).
pub fn size_words(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1000.0;
    let mut unit = 0;
    while v >= 999.95 && unit + 1 < UNITS.len() {
        v /= 1000.0;
        unit += 1;
    }
    if v < 10.0 { format!("{v:.1} {}", UNITS[unit]) } else { format!("{v:.0} {}", UNITS[unit]) }
}

impl App {
    /// `f`: download a file of the selected message (the one it has, or the one picked).
    pub(super) fn open_files(&mut self, now: Instant) {
        let Some((workspace, files)) = self.selected_files() else {
            return self.info(Msg::Label(Label::FileNone), now);
        };
        let can: Vec<File> = files.into_iter().filter(|f| f.download.is_some()).collect();
        match can.len() {
            0 => self.warn(Msg::Label(Label::FileNotDownloadable), now),
            1 => self.download(&workspace, &can[0], now),
            n => {
                let hints = hint_letters(n);
                self.file_picker = Some(FilePicker { workspace, files: can, hints, typed: String::new(), cursor: 0 });
            }
        }
    }

    /// The workspace and the files of the selected message of the active pane.
    fn selected_files(&self) -> Option<(WorkspaceId, Vec<File>)> {
        let p = self.work.pane(self.work.active()?)?;
        let tl = self.work.timeline(p);
        let (a, _) = p.range(tl)?;
        let files = tl.items[a].files.clone();
        (!files.is_empty()).then(|| (p.target.workspace().clone(), files))
    }

    /// Ask the backend for `file` of `workspace`, saved in the downloads folder.
    fn download(&mut self, workspace: &WorkspaceId, file: &File, now: Instant) {
        let Some(dir) = self.downloads.dir.clone() else {
            return self.warn(Msg::Label(Label::FileNoFolder), now);
        };
        let command = Command::Download { file: file.clone(), dir, max: self.downloads.max };
        let id = self.work.requests.ask(Some(workspace.clone()), command);
        let name = crate::text::clip(file.name.line().as_str(), NAME_SAID);
        self.downloads.going.push(Going { id, file: file.id.clone(), name: name.clone() });
        self.say_download(Msg::FileDownloading { name }, Level::Info, now);
    }

    /// Say `msg` about a download in place of what was said of downloads before.
    fn say_download(&mut self, msg: Msg, level: Level, now: Instant) {
        self.status.forget(now, |m| {
            matches!(m, Msg::FileDownloading { .. } | Msg::FileProgress { .. } | Msg::FileSaved { .. })
        });
        self.status.show(msg, level, now);
    }

    /// How much of the file download `id` asked for has come.
    pub(super) fn on_progress(
        &mut self,
        id: Generation,
        file: &FileId,
        got: u64,
        total: Option<u64>,
        now: Instant,
    ) -> bool {
        let Some(g) = self.downloads.going.iter().find(|g| g.id == id && &g.file == file) else { return false };
        let name = g.name.clone();
        let msg = match total.filter(|t| *t > 0) {
            Some(t) => Msg::FileProgress { name, percent: (got.min(t) * 100 / t).to_string() },
            None => Msg::FileDownloading { name },
        };
        self.say_download(msg, Level::Info, now);
        true
    }

    /// Download `id` saved its file as `path`: it is marked as from the internet, and offered to
    /// open when it is of a kind a viewer shows (and nothing else is asked or picked now).
    pub(super) fn on_downloaded(&mut self, id: Generation, path: PathBuf, now: Instant) -> bool {
        let Some(i) = self.downloads.going.iter().position(|g| g.id == id) else { return false };
        let g = self.downloads.going.remove(i);
        self.effects.push(Effect::Quarantine(path.clone()));
        let saved =
            path.file_name().map_or_else(|| g.name.clone(), |n| crate::text::clip(&n.to_string_lossy(), NAME_SAID));
        if !slakio_core::download::openable(&path) {
            self.say_download(Msg::FileSavedOnly { name: saved }, Level::Info, now);
            return true;
        }
        self.say_download(Msg::FileSaved { name: saved }, Level::Info, now);
        let free = self.dialog.is_none() && self.layers(None).is_empty() && self.picker.is_none();
        if free {
            self.downloads.saved = Some(path);
            self.dialog = Some(Dialog::new(Question::File));
        }
        true
    }

    /// Request `id` failed: when it was a download, the status line says why. `true` when it was.
    pub(super) fn download_failed(&mut self, id: Generation, kind: &FailKind, now: Instant) -> bool {
        let Some(i) = self.downloads.going.iter().position(|g| g.id == id) else { return false };
        self.downloads.going.remove(i);
        let why = match kind {
            FailKind::Api { code } if code == "redirected_elsewhere" => Label::FileWhyElsewhere,
            FailKind::Api { code } if code == "not_a_file" => Label::FileWhyPage,
            FailKind::Api { code } if code == "file_not_available" => Label::FileWhyGone,
            FailKind::Api { code } if code == "file_not_downloadable" => Label::FileNotDownloadable,
            kind => super::failure_label(kind),
        };
        let why = self.i18n.label(why).to_string();
        self.say_download(Msg::FileFailed { why }, Level::Warning, now);
        true
    }

    /// The answer to "open this file?": yes hands it to the binary.
    pub(super) fn answer_file(&mut self, yes: bool) {
        if let Some(path) = self.downloads.saved.take().filter(|_| yes) {
            self.effects.push(Effect::OpenFile(path));
        }
    }

    /// The file the question offers to open, while it does.
    pub fn saved_file(&self) -> Option<&std::path::Path> {
        self.downloads.saved.as_deref()
    }

    /// The files being picked, while they are.
    pub fn file_picker(&self) -> Option<&FilePicker> {
        self.file_picker.as_ref()
    }

    /// A key while a file is picked: a letter of the labels, else the list's key ([`Ctx::Links`]);
    /// any other key closes it.
    pub(super) fn file_key(&mut self, key: KeyChord, now: Instant) {
        let plain = !key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let letter = match key.code {
            KeyCode::Char(c) if plain && LETTERS.contains(&c.to_ascii_lowercase()) => Some(c.to_ascii_lowercase()),
            _ => None,
        };
        let Some(mut p) = self.file_picker.take() else { return };
        let Some(c) = letter else {
            let bound = self.keymap.bindings(Ctx::Links).find(|(k, _)| *k == [key]).map(|(_, a)| a);
            let last = p.files.len().saturating_sub(1);
            match bound {
                Some(Action::Links(LinksAction::Down)) => p.cursor = (p.cursor + 1).min(last),
                Some(Action::Links(LinksAction::Up)) => p.cursor = p.cursor.saturating_sub(1),
                Some(Action::Links(LinksAction::Back)) => {
                    p.typed.pop();
                }
                Some(Action::Links(LinksAction::Open)) => return self.pick_file(&p, p.cursor, now),
                Some(Action::Links(LinksAction::List)) => {}
                _ => return,
            }
            self.file_picker = Some(p);
            return;
        };
        p.typed.push(c);
        if let Some(i) = p.hints.iter().position(|h| *h == p.typed) {
            return self.pick_file(&p, i, now);
        }
        if p.hints.iter().any(|h| h.starts_with(&p.typed)) {
            self.file_picker = Some(p);
        }
    }

    fn pick_file(&mut self, p: &FilePicker, i: usize, now: Instant) {
        if let Some(f) = p.files.get(i) {
            self.download(&p.workspace.clone(), &f.clone(), now);
        }
    }

    /// Where the list is, and its rows, while it shows.
    pub(crate) fn file_rects(&self) -> Option<(Rect, Rect)> {
        let p = self.file_picker.as_ref()?;
        let body = Rect { height: self.size.height.saturating_sub(1), ..self.size };
        let rect = list_rect(body, p.files.len());
        Some((rect, crate::screen::inner(rect)))
    }

    /// The first row of the list shown, `rows` of them showing.
    pub fn file_top(&self, rows: usize) -> usize {
        let Some(p) = self.file_picker.as_ref() else { return 0 };
        (p.cursor + 1).saturating_sub(rows).min(p.files.len().saturating_sub(rows))
    }

    /// The mouse while files are picked: a click on a row downloads it, the wheel moves the
    /// cursor, a press outside the list closes it, a move changes nothing. `None` when no list
    /// shows.
    pub(super) fn file_mouse(&mut self, kind: MouseEventKind, at: Position, now: Instant) -> Option<bool> {
        let (rect, inner) = self.file_rects()?;
        let rows = usize::from(inner.height);
        let top = self.file_top(rows);
        let p = self.file_picker.as_mut()?;
        let last = p.files.len().saturating_sub(1);
        match kind {
            MouseEventKind::Moved | MouseEventKind::Up(_) | MouseEventKind::Drag(_) => return Some(false),
            MouseEventKind::ScrollDown => p.cursor = (p.cursor + 1).min(last),
            MouseEventKind::ScrollUp => p.cursor = p.cursor.saturating_sub(1),
            MouseEventKind::Down(MouseButton::Left) if inner.contains(at) => {
                let i = top + usize::from(at.y - inner.y);
                let p = self.file_picker.take()?;
                if i < p.files.len() {
                    self.pick_file(&p, i, now);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if rect.contains(at) => return Some(false),
            _ => self.file_picker = None,
        }
        Some(true)
    }

    /// The keys the status line offers while files are picked.
    pub(super) fn file_hints(&self) -> Option<Vec<(String, Label)>> {
        self.file_picker.as_ref()?;
        Some(vec![
            ("j/k".to_string(), Label::HintMove),
            ("Enter".to_string(), Label::HintDownload),
            ("Esc".to_string(), Label::HintCancel),
        ])
    }
}

/// The most cells of a file's name the status line says.
const NAME_SAID: usize = 24;

#[cfg(test)]
mod tests;
