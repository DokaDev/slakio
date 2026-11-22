//! `:theme update`: add to a theme file the tokens it does not list (neither set nor in a
//! commented-out `# <token> = …` line of its section, as in a file written before they existed),
//! commented out with the value each takes now, at the end of their section, in the file's own
//! line breaks. No line of the file changes; the file is replaced (0600, at once) only when it
//! reads exactly as before and has not changed since it was read.

use super::{AuthorError, COLOR_ABOUT, STYLE_ABOUT, color_value, style_value};
use crate::theme::custom::from_spec;
use crate::theme::{Look, NAMES};
use slakio_core::fsutil;
use slakio_core::theme_file::{self, ThemeSpec};
use std::path::{Path, PathBuf};

/// What `:theme update` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Updated {
    /// `count` tokens were added, commented out.
    Added { path: PathBuf, count: usize },
    /// The file lists every token already.
    Complete { path: PathBuf },
}

/// `:theme update <name>`: add to `<dir>/<name>.toml` every token it does not list (see the
/// module's text), with the value it takes on `look`. Refused, the file left as it is, when it
/// cannot be read, has an error, may not be written, changed since it was read, or would not
/// read exactly as it did.
pub fn update(dir: &Path, name: &str, look: Look) -> Result<Updated, AuthorError> {
    if NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        return Err(AuthorError::NotAFile(name.to_string()));
    }
    let (text, spec) = theme_file::read_text(dir, name).map_err(AuthorError::Source)?;
    let path = theme_file::path(dir, name);
    let th = from_spec(name, &spec, Look { no_color: false, ..look });
    let (colors, styles) = unlisted(&text, &spec);
    if colors.is_empty() && styles.is_empty() {
        return Ok(Updated::Complete { path });
    }
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut blocks = [String::new(), String::new()];
    let [color_block, style_block] = &mut blocks;
    for (block, tokens, about, is_color) in
        [(color_block, &colors, COLOR_ABOUT, true), (style_block, &styles, STYLE_ABOUT, false)]
    {
        if tokens.is_empty() {
            continue;
        }
        block.push_str(
            "\n# Added by :theme update: tokens this file did not list, with the value each takes now,\n\
             # commented out. Remove the \"# \" in front of a key to set it.\n",
        );
        for &token in tokens {
            let value = if is_color { color_value(&th, token) } else { style_value(&th, token).map(|(v, _)| v) };
            let value = value.ok_or(AuthorError::Unwritable { token })?;
            let about = about.iter().find(|(t, _)| *t == token).map_or("", |(_, a)| a).replace('`', "");
            block.push_str(&format!("\n# {about}\n# {token} = {value}\n"));
        }
        *block = block.replace('\n', nl);
    }
    let new = insert_blocks(&text, &blocks[0], &blocks[1], nl);
    // It reads as the file did, and lists every token now.
    let same = theme_file::parse(&new)
        .is_ok_and(|n| same_meaning(&n, &spec) && unlisted(&new, &n) == (Vec::new(), Vec::new()));
    if !same {
        return Err(AuthorError::Changed(path));
    }
    write_checked(&path, &text, &new)?;
    Ok(Updated::Added { path, count: colors.len() + styles.len() })
}

/// Replace the file at `path`, read as `text`, with `new`: refused when it may not be written
/// (a read-only file, a link or a pipe is not replaced, and none is waited on) or no longer
/// reads as `text`. The new file is renamed into place, so a link put there since is replaced,
/// never followed.
pub(super) fn write_checked(path: &Path, text: &str, new: &str) -> Result<(), AuthorError> {
    if !fsutil::writable_plain(path) {
        return Err(AuthorError::Write(path.to_path_buf()));
    }
    if fsutil::read_plain(path, theme_file::MAX_BYTES).ok().as_deref() != Some(text.as_bytes()) {
        return Err(AuthorError::Changed(path.to_path_buf()));
    }
    fsutil::atomic_replace_private_nofollow(path, new.as_bytes()).map_err(|_| AuthorError::Write(path.to_path_buf()))
}

/// The two read files set the same (the line `extends` is on aside).
fn same_meaning(a: &ThemeSpec, b: &ThemeSpec) -> bool {
    let base = |s: &ThemeSpec| s.extends.as_ref().map(|e| e.0.clone());
    ThemeSpec { extends: None, ..a.clone() } == ThemeSpec { extends: None, ..b.clone() } && base(a) == base(b)
}

/// The colour and style tokens the theme file `text` (read as `spec`) does not list: neither set
/// nor in a commented-out `# <token> = …` line of its own section.
pub fn unlisted(text: &str, spec: &ThemeSpec) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut commented: Vec<(String, &str)> = Vec::new();
    let mut section = String::new();
    for line in text.lines() {
        if let Some(name) = table(line) {
            section = name;
            continue;
        }
        let Some(rest) = line.trim_start_matches('\u{feff}').trim_start().strip_prefix('#') else { continue };
        let rest = rest.trim_start();
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
        if end > 0 && rest[end..].trim_start().starts_with('=') {
            commented.push((section.clone(), &rest[..end]));
        }
    }
    let listed = |sec: &str, t: &str| commented.iter().any(|(s, c)| s == sec && *c == t);
    let colors = COLOR_ABOUT.iter().map(|(t, _)| *t).filter(|t| {
        let set = match *t {
            "workspaces" => spec.workspaces.is_some(),
            "avatars" => spec.avatars.is_some(),
            "avatar_tint" => spec.avatar_tint.is_some(),
            "dim" => spec.dim.is_some(),
            _ => spec.colors.iter().any(|(c, _)| c == t),
        };
        !set && !listed("colors", t)
    });
    let styles = STYLE_ABOUT
        .iter()
        .map(|(t, _)| *t)
        .filter(|t| !spec.styles.iter().any(|(s, _)| s == t) && !listed("styles", t));
    (colors.collect(), styles.collect())
}

/// The table a line opens: `[name]` (also quoted) with nothing but a comment after it.
fn table(l: &str) -> Option<String> {
    let t = l.trim_start_matches('\u{feff}').trim();
    let inner = t.strip_prefix('[').filter(|r| !r.starts_with('['))?;
    let (name, rest) = inner.split_once(']')?;
    let rest = rest.trim_start();
    let name = name.trim();
    let name = ['"', '\''].iter().find_map(|q| name.strip_prefix(*q)?.strip_suffix(*q)).unwrap_or(name);
    (rest.is_empty() || rest.starts_with('#')).then(|| name.to_string())
}

/// `text` with `colors` at the end of its `[colors]` table and `styles` at the end of its
/// `[styles]` (before the blank lines in front of the next table); a table it does not have is
/// added at the end. The lines of `text` stay as they are, in order (a last line gets its line
/// break, `nl`).
fn insert_blocks(text: &str, colors: &str, styles: &str, nl: &str) -> String {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    if let Some(last) = lines.last_mut()
        && !last.ends_with('\n')
    {
        last.push_str(nl);
    }
    let end_of = |name: &str| -> Option<usize> {
        let start = lines.iter().position(|l| table(l).as_deref() == Some(name))?;
        let mut end = (start + 1..lines.len()).find(|&i| table(&lines[i]).is_some()).unwrap_or(lines.len());
        while end > start + 1 && lines[end - 1].trim().is_empty() {
            end -= 1;
        }
        Some(end)
    };
    let mut at: Vec<(usize, usize, String)> = Vec::new();
    let mut tail = String::new();
    for (order, (name, block)) in [("colors", colors), ("styles", styles)].into_iter().enumerate() {
        if block.is_empty() {
            continue;
        }
        match end_of(name) {
            Some(end) => at.push((end, order, block.to_string())),
            None => tail.push_str(&format!("{nl}[{name}]{nl}{block}")),
        }
    }
    // From the last place up, so the places before stay where they are.
    at.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
    for (end, _, block) in at {
        lines.insert(end, block);
    }
    let mut out = lines.concat();
    out.push_str(&tail);
    out
}
