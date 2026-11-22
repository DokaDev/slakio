//! File helpers shared by everything slakio writes.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Replace the file at `path` with `bytes` so that a crash or a full disk never leaves a
/// half-written file:
///
/// 1. a symbolic link (dotfiles) is written through to its target;
/// 2. the bytes go to `.<name>.tmp-<pid>` in the same directory, which gets the original
///    file's permissions (Unix mode), and are flushed to disk;
/// 3. the temporary file is renamed over the target (`rename` replaces it on Windows too),
///    and on Unix the directory is synced so the rename itself survives a crash;
/// 4. on failure the temporary file is removed and the error returned; the target is untouched.
///
/// Missing parent directories are created.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomically(path, bytes, false)
}

/// [`atomic_write`] for files that hold secrets: the file is always written with mode 0600
/// (Unix), whatever the permissions of the file it replaces. On Windows it is the same as
/// [`atomic_write`] (the file inherits the directory's ACL).
pub fn atomic_write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomically(path, bytes, true)
}

/// [`atomic_write_private`] that never writes through a symbolic link: a link at `path` is
/// replaced by the new file (a rename replaces the link, never what it points to), so a link
/// put there after the file was read is not followed. The folder must be there already.
pub fn atomic_replace_private_nofollow(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = match path.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(d) => d.to_path_buf(),
        None => PathBuf::from("."),
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = write_then_rename(&tmp, path, bytes, true);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    sync_dir(&dir);
    Ok(())
}

/// The plain file at `path` may be written: it opens for writing (nothing is written) without
/// following a link and without waiting on it (Unix), and is a plain file as opened.
pub fn writable_plain(path: &Path) -> bool {
    let mut opts = fs::OpenOptions::new();
    opts.write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut opts, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    opts.open(path).and_then(|f| f.metadata()).is_ok_and(|m| m.is_file())
}

fn write_atomically(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    let target = write_target(path)?;
    let dir = match target.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(d) => d.to_path_buf(),
        None => PathBuf::from("."),
    };
    // A folder made for a private file is private too.
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    }
    b.create(&dir)?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = write_then_rename(&tmp, &target, bytes, private);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    sync_dir(&dir);
    Ok(())
}

fn write_then_rename(tmp: &Path, target: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    // Made new, never through what is already there (a link put in its place is not
    // followed); one left by an earlier run of the same id is removed first.
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    // A private file is never readable by others, not even for a moment.
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    }
    let mut f = match opts.open(tmp) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(tmp)?;
            opts.open(tmp)?
        }
        opened => opened?,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if private {
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        } else if let Ok(meta) = fs::metadata(target) {
            f.set_permissions(meta.permissions())?;
        }
    }
    #[cfg(not(unix))]
    let _ = (target, private);
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(tmp, target)
}

/// Write a new file at `path`, never replacing anything already there (an `AlreadyExists`
/// error then). The bytes go to a temporary file first, which is flushed and then hard-linked
/// into place, so the file appears complete or not at all and an existing one is never
/// touched. On a file system without hard links the file is created with `create_new`
/// (`O_EXCL`) and written in place (removed again when that fails). Missing parent
/// directories are created.
pub fn create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    make_new(path, bytes, false)
}

/// [`create_new`] for a file only its owner may read: mode 0600 (Unix) from the moment it is
/// made, whatever the umask. On Windows it is the same as [`create_new`].
pub fn create_new_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    make_new(path, bytes, true)
}

fn make_new(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    let opts = fs::OpenOptions::new();
    #[cfg(unix)]
    let opts = {
        let mut opts = opts;
        if private {
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        }
        opts
    };
    #[cfg(not(unix))]
    let _ = private;
    let dir = match path.parent().filter(|d| !d.as_os_str().is_empty()) {
        Some(d) => d.to_path_buf(),
        None => PathBuf::from("."),
    };
    fs::create_dir_all(&dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.new-{}", std::process::id()));
    let result = (|| {
        let mut f = open_new(opts.clone().write(true).create_new(true), &tmp)?;
        // Whatever the umask: private.
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        match fs::hard_link(&tmp, path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(e),
            // No hard links here: create it exclusively and write it in place.
            Err(_) => {
                let mut f = opts.clone().write(true).create_new(true).open(path)?;
                let written = f.write_all(bytes).and_then(|()| f.sync_all());
                if written.is_err() {
                    let _ = fs::remove_file(path);
                }
                written
            }
        }
    })();
    let _ = fs::remove_file(&tmp);
    if result.is_ok() {
        sync_dir(&dir);
    }
    result
}

/// `path` opened with `opts` (which make it new, `create_new`): never through what is there
/// already (a link put in its place is not followed). One left there (by an earlier run of the
/// same id, or planted) is removed, the link itself and not what it points to, and the open
/// tried again, a few times at most.
fn open_new(opts: &fs::OpenOptions, path: &Path) -> io::Result<fs::File> {
    let mut tries = 0;
    loop {
        match opts.open(path) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && tries < 3 => {
                tries += 1;
                fs::remove_file(path)?;
            }
            opened => return opened,
        }
    }
}

/// Rename `from` to `to`, never replacing anything at `to` (an `AlreadyExists` error then;
/// `fs::rename` alone would silently replace a file). A file is hard-linked to its new name
/// and then unlinked from the old one; a folder, a symbolic link, or a file on a file system
/// without hard links is renamed by the system's own rename that refuses a taken name
/// (`renamex_np(RENAME_EXCL)` on macOS, `renameat2(RENAME_NOREPLACE)` on Linux), never by a
/// check and then a rename. Where there is no such rename (or the file system refuses it), it
/// fails rather than risk replacing something.
pub fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(from)?.is_file() {
        match fs::hard_link(from, to) {
            Ok(()) => {
                if let Err(e) = fs::remove_file(from) {
                    let _ = fs::remove_file(to);
                    return Err(e);
                }
                if let Some(d) = to.parent() {
                    sync_dir(d);
                }
                return Ok(());
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(e),
            Err(_) => {}
        }
    }
    rename_noreplace(from, to)?;
    if let Some(d) = to.parent() {
        sync_dir(d);
    }
    Ok(())
}

/// The system's rename that refuses a taken name.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

/// None here: refused.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn rename_noreplace(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no rename that refuses a taken name"))
}

/// The file a write of `path` goes to: `path` itself, or the file a symbolic link at `path`
/// points to (a dotfiles link is written through, never replaced by a plain file). A link that
/// cannot be resolved for another reason than a missing target is an error.
fn write_target(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(t) => Ok(t),
        Err(e) => match fs::symlink_metadata(path) {
            // A link to a file that does not exist yet: create that file.
            Ok(m) if m.file_type().is_symlink() && e.kind() == io::ErrorKind::NotFound => {
                let to = fs::read_link(path)?;
                Ok(match path.parent() {
                    Some(dir) if to.is_relative() => dir.join(to),
                    _ => to,
                })
            }
            Ok(m) if m.file_type().is_symlink() => Err(e),
            // Not there yet (or not a link): write the path itself.
            _ => Ok(path.to_path_buf()),
        },
    }
}

/// Where to move a file aside as a backup: `path` when nothing is there, else the first free
/// `<path>.<n>` (n = 2, 3, …), so an older backup is never replaced. A name that cannot be
/// looked at counts as taken. Move with [`rename_new`], which refuses a taken name anyway.
pub fn free_backup(path: &Path) -> PathBuf {
    let taken = |p: &Path| !matches!(fs::symlink_metadata(p), Err(e) if e.kind() == io::ErrorKind::NotFound);
    if !taken(path) {
        return path.to_path_buf();
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    (2..).map(|n| path.with_file_name(format!("{name}.{n}"))).find(|p| !taken(p)).unwrap_or_default()
}

/// Make `dir` (and the folders above it that are missing) a folder only its owner may enter
/// (0700 on Unix); one there already that others may enter is made private too, when it is the
/// user's (only its owner may change its mode; otherwise it stays as it is).
pub fn private_dir(dir: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(dir).is_ok_and(|m| m.permissions().mode() & 0o077 != 0) {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
    }
    Ok(())
}

/// [`private_dir`] for a folder that may be a symbolic link (the user's dotfiles), to write files
/// in: a folder made here, or there already and not a link, is made private as [`private_dir`]
/// does, but what a link points to is never changed. Hands back whether files may be written
/// there: the folder (a link's target) is the user's own and no one else may write in it (Unix;
/// elsewhere it always may), so no one else can put a file or a link in place of one written.
pub fn own_private_dir(dir: &Path) -> io::Result<bool> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)?;
    #[cfg(unix)]
    let ok = {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        // Opened without following a link: only a folder that is itself there is changed.
        let mut opts = fs::OpenOptions::new();
        opts.read(true).custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY);
        if let Ok(d) = opts.open(dir)
            && d.metadata().is_ok_and(|m| m.is_dir() && m.mode() & 0o077 != 0)
        {
            let _ = d.set_permissions(fs::Permissions::from_mode(0o700));
        }
        let m = fs::metadata(dir)?;
        m.is_dir() && is_mine(m.uid()) && m.mode() & 0o022 == 0
    };
    #[cfg(not(unix))]
    let ok = true;
    Ok(ok)
}

/// The user id `uid` is the one slakio runs as.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn is_mine(uid: u32) -> bool {
    rustix::process::geteuid().as_raw() == uid
}

/// Not known here: taken as not (fail closed).
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn is_mine(_uid: u32) -> bool {
    false
}

/// The bytes of the plain file at `path`, at most `max` of them. A symbolic link, a pipe or
/// anything but a plain file is refused without waiting on it (it is opened without following
/// a link and without blocking, then looked at as opened), and so is a file longer than `max`.
pub fn read_plain(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut opts, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let f = opts.open(path)?;
    if !f.metadata()?.is_file() {
        return Err(io::Error::other("not a plain file"));
    }
    let mut bytes = Vec::new();
    f.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::other(format!("over {max} bytes")));
    }
    Ok(bytes)
}

/// Make a rename in `dir` durable (Unix). Best effort: some file systems refuse it, and the
/// data itself is already on disk.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests;
