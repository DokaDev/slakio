//! The files a message shares: downloaded (`f`, `:download`), saved, then opened when asked.
//!
//! `f` on a message with one file that can be downloaded asks the backend for it at once; with
//! several, a list shows them (their letters as the link picker's, its keys the `links`
//! context's: `j`/`k`, `Enter`, `Esc`; a click on a row, the wheel), and the one picked is asked
//! for. A file hosted elsewhere, or none, is said. Downloading is a read: nothing changes in
//! Slack, so a read-only workspace downloads too.
//!
//! While a file comes, the status line says so until it ends ([`App::notice`]: a notice covers
//! it for its time, then it is back): that it is waited for, how much has come (with two on their
//! way, that two are) and the key that stops them (`F`, `:download cancel`); a file already on its
//! way is not asked for again. What is said of a download replaces what was said of one before,
//! a failure too. Once saved, a file of a kind a viewer shows
//! ([`slakio_core::download::openable`]) is offered to open (`dialog::Question::File`, the safe
//! answer focused: nothing opens by itself); any other kind is only saved, and the status line
//! says so. Every file saved was marked as downloaded from the internet before it got its name
//! ([`slakio_core::download`]); one that could not be marked, and the demo's invented files,
//! are never offered to open.

use super::dialog::{Dialog, Question};
use super::model::Row;
use super::overlay::Layer;
use super::picker::{LETTERS, hint_letters, list_rect};
use super::status::{INFO_FOR, Level, Notice};
use super::{App, Effect};
use crate::action::{Action, LinksAction};
use crate::keymap::{Ctx, KeyChord};
use crate::text::clip_name;
use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use slakio_core::backend::{Command, FailKind, Generation};
use slakio_core::i18n::{Label, Msg};
use slakio_core::images::Source;
use slakio_core::model::{ConversationKind, File, FileId, WorkspaceId};
use std::path::PathBuf;
use std::time::Instant;

/// Where files are saved and how large one may be (set by the binary from the config).
#[derive(Clone, Debug, Default)]
pub struct Downloads {
    /// The folder; `None` when there is none (no home directory, nothing configured).
    pub dir: Option<PathBuf>,
    /// The most bytes of a file.
    pub max: u64,
    /// The home folder, said `~` where a saved file's folder starts with it.
    pub home: Option<PathBuf>,
    /// The downloads asked for and not answered yet.
    going: Vec<Going>,
    /// The file saved that the question offers to open.
    saved: Option<PathBuf>,
    /// The downloads on their way were told to stop.
    stopping: bool,
    /// The names of the downloads stopped, until all of them are said at once.
    stopped: Vec<String>,
}

/// A download asked for: its request, its workspace, its file and the name said for it.
#[derive(Clone, Debug)]
struct Going {
    id: Generation,
    workspace: WorkspaceId,
    file: FileId,
    name: String,
    /// The bytes come so far, and how many there are when that is known.
    got: u64,
    total: Option<u64>,
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
    /// After a frame: ask the backend for the pictures it would have shown and does not have
    /// ([`crate::images::Images::ask`]), and for the lists of the workspaces' own emoji it showed
    /// a shortcode of, once booted ([`crate::images::custom::Customs::ask`]), and then for the
    /// photos of the DM rows out of view ([`crate::images::Images::offer_ahead`]). `true` when
    /// something was asked.
    pub fn ask_images(&mut self, now: Instant) -> bool {
        let model = &self.model;
        self.images.customs.booted_only(|ws| model.is_fresh(ws));
        let lists = self.images.customs.ask(&mut self.work.requests, now);
        // Under a popup that covers the messages, nothing moves.
        let covers = |l: &Layer| matches!(l, Layer::Dialog | Layer::Settings | Layer::Help | Layer::Files);
        self.images.cover(self.layers(None).iter().any(covers));
        let photos = self.settings.photos && self.settings.avatars;
        let dms = if photos { self.dm_photos() } else { Vec::new() };
        self.images.offer_ahead(photos, dms);
        let asked = self.images.ask(&mut self.work.requests, now) | lists;
        // Looks in the cache on disk run off the loop's thread, as decodes do.
        self.effects.extend(self.images.take_lookups().into_iter().map(Effect::Decode));
        asked
    }

    /// The photo of each 1:1 DM's peer the list lists, top down, out of view too (to ask for
    /// ahead, [`crate::images::Images::offer_ahead`]).
    fn dm_photos(&self) -> Vec<(WorkspaceId, Source)> {
        let rows = self.list_rows().into_iter().filter_map(|r| match r {
            Row::Conversation(i) => Some(self.model.conversation(i)),
            _ => None,
        });
        rows.filter_map(|c| match &c.kind {
            ConversationKind::Dm { user } => {
                Some((c.workspace.clone(), Source::Avatar(self.model.user(&c.workspace, user)?.avatar.clone()?)))
            }
            _ => None,
        })
        .collect()
    }

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

    /// Ask the backend for `file` of `workspace`, saved in the downloads folder (not again
    /// while it is on its way).
    fn download(&mut self, workspace: &WorkspaceId, file: &File, now: Instant) {
        let Some(dir) = self.downloads.dir.clone() else {
            return self.warn(Msg::Label(Label::FileNoFolder), now);
        };
        let name = crate::text::clip_name(file.name.line().as_str(), NAME_SAID);
        if self.downloads.going.iter().any(|g| g.file == file.id && &g.workspace == workspace) {
            return self.info(Msg::FileAlready { name }, now);
        }
        let command = Command::Download { file: file.clone(), dir, max: self.downloads.max };
        let id = self.work.requests.ask(Some(workspace.clone()), command);
        let going = Going { id, workspace: workspace.clone(), file: file.id.clone(), name, got: 0, total: None };
        self.downloads.going.push(going);
        self.downloads.stopping = false;
        // What was said of a download before is over: the line of this one shows.
        self.status.forget(now, about_downloads);
    }

    /// The line the status line shows while downloads are on their way: one by its name (waited
    /// for until its first bytes, then how much of it has come), several by their number, and
    /// the key that stops them; once told to stop, that they are stopping.
    fn download_line(&self) -> Option<Msg> {
        let stop = self.stop_key();
        Some(match self.downloads.going.as_slice() {
            [] => return None,
            _ if self.downloads.stopping => Msg::Label(Label::FileCancelling),
            [one] => match (one.got, one.total) {
                (0, _) => Msg::FileWaiting { name: one.name.clone(), stop },
                (got, Some(t)) if t > 0 => {
                    Msg::FileProgress { name: one.name.clone(), percent: (got.min(t) * 100 / t).to_string(), stop }
                }
                _ => Msg::FileDownloading { name: one.name.clone(), stop },
            },
            many => Msg::FileDownloadingMany { count: many.len() as u64, stop },
        })
    }

    /// What the status line's message slot shows at `now`: the notice showing, else while
    /// downloads are on their way, their line (it stands until the last one ends).
    pub fn notice(&self, now: Instant) -> Option<Notice> {
        self.status.notice(now).cloned().or_else(|| {
            Some(Notice { msg: self.download_line()?, level: Level::Info, until: now + INFO_FOR, about: None })
        })
    }

    /// The key that stops the downloads where the panes' keys work, else the command.
    fn stop_key(&self) -> String {
        let action = Action::Pane(crate::action::PaneAction::FilesCancel);
        crate::keymap::hints::key_label(&self.keymap, action, Ctx::PaneNormal)
            .unwrap_or_else(|| ":download cancel".to_string())
    }

    /// Say `msg` about a download in place of what was said of downloads before (a failure
    /// too: the latest says how things are); after another warning showing, when there is one.
    fn say_download(&mut self, msg: Msg, level: Level, now: Instant) {
        self.status.forget(now, about_downloads);
        if self.status.notice(now).is_some_and(|n| n.level > level) {
            return self.status.queue(msg, level, now);
        }
        self.status.show(msg, level, now);
    }

    /// `F`, `:download cancel`: stop every download on its way. Each one's own answer says
    /// whether it stopped (one already being put in place is saved all the same).
    pub(super) fn cancel_downloads(&mut self, now: Instant) {
        if self.downloads.going.is_empty() {
            return self.info(Msg::Label(Label::FileNoDownload), now);
        }
        let asked: Vec<(Generation, WorkspaceId)> =
            self.downloads.going.iter().map(|g| (g.id, g.workspace.clone())).collect();
        for (request, workspace) in asked {
            self.work.requests.ask(Some(workspace), Command::Cancel { request });
        }
        self.downloads.stopping = true;
        // Their line says they are stopping.
        self.status.forget(now, about_downloads);
    }

    /// How much of the file download `id` asked for has come.
    pub(super) fn on_progress(&mut self, id: Generation, file: &FileId, got: u64, total: Option<u64>) -> bool {
        let Some(g) = self.downloads.going.iter_mut().find(|g| g.id == id && &g.file == file) else {
            return false;
        };
        (g.got, g.total) = (got, total);
        true
    }

    /// Download `id` saved its file as `path`, marked as from the internet unless `unmarked`: it
    /// is offered to open when it is of a kind a viewer shows, was marked and is not the demo's
    /// (and nothing else is asked or picked now).
    pub(super) fn on_downloaded(&mut self, id: Generation, path: PathBuf, unmarked: bool, now: Instant) -> bool {
        let Some(i) = self.downloads.going.iter().position(|g| g.id == id) else { return false };
        let g = self.downloads.going.remove(i);
        self.downloads.stopped.clear();
        let name = path.file_name().map_or_else(|| g.name.clone(), |n| clip_name(&n.to_string_lossy(), NAME_SAID));
        let folder = path.parent().map(|d| self.folder_said(d)).unwrap_or_default();
        if unmarked {
            self.say_download(Msg::FileSavedUnmarked { name, folder }, Level::Warning, now);
            return true;
        }
        if !slakio_core::download::openable(&path) {
            self.say_download(Msg::FileSavedOnly { name, folder }, Level::Info, now);
            return true;
        }
        if self.backend.is_some_and(|c| c.demo) {
            self.say_download(Msg::FileSavedDemo { name, folder }, Level::Info, now);
            return true;
        }
        self.say_download(Msg::FileSaved { name, folder }, Level::Info, now);
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
        let g = self.downloads.going.remove(i);
        if *kind == FailKind::Cancelled {
            // Those stopped at once are said at once, when the last one has stopped.
            self.downloads.stopped.push(g.name);
            if self.downloads.going.is_empty() {
                let msg = match std::mem::take(&mut self.downloads.stopped).as_slice() {
                    [one] => Msg::FileCancelled { name: one.clone() },
                    many => Msg::FileCancelledMany { count: many.len() as u64 },
                };
                self.say_download(msg, Level::Info, now);
            }
            return true;
        }
        self.downloads.stopped.clear();
        if *kind == FailKind::TooLarge {
            let why = self.i18n.msg(&Msg::FileWhyTooLarge { limit: (self.downloads.max / 1_000_000).to_string() });
            self.say_download(Msg::FileFailed { why: why.to_string() }, Level::Warning, now);
            return true;
        }
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

    /// Folder `dir` as the status line says it: the home folder as `~`.
    fn folder_said(&self, dir: &std::path::Path) -> String {
        match self.downloads.home.as_deref().and_then(|h| dir.strip_prefix(h).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
            Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
            None => dir.display().to_string(),
        }
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

/// Notice `m` says how a download went (or that one is on its way, or none is).
fn about_downloads(m: &Msg) -> bool {
    matches!(
        m,
        Msg::FileAlready { .. }
            | Msg::FileCancelled { .. }
            | Msg::FileCancelledMany { .. }
            | Msg::FileSaved { .. }
            | Msg::FileSavedOnly { .. }
            | Msg::FileSavedUnmarked { .. }
            | Msg::FileSavedDemo { .. }
            | Msg::FileFailed { .. }
            | Msg::Label(Label::FileNoDownload)
    )
}

#[cfg(test)]
mod tests;
