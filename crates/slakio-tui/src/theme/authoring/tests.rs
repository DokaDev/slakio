use super::*;
use crate::theme::{BUILTINS, Background, load};
use std::path::PathBuf;

/// A scratch themes folder of its own (not made: the writers make it), removed when dropped.
struct Dir(PathBuf, PathBuf);

impl Dir {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("slakio-author-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dir = root.join("themes");
        Self(dir, root)
    }

    fn text(&self, name: &str) -> String {
        std::fs::read_to_string(theme_file::path(&self.0, name)).unwrap()
    }

    fn put(&self, name: &str, text: &str) -> PathBuf {
        std::fs::create_dir_all(&self.0).unwrap();
        let path = theme_file::path(&self.0, name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

const LOOKS: [Look; 3] = [
    Look { truecolor: true, background: Background::Dark, no_color: false },
    Look { truecolor: false, background: Background::Light, no_color: false },
    Look { truecolor: true, background: Background::Unknown, no_color: false },
];

/// `th` as the theme file `name` draws it.
fn named(th: &Theme, name: &str) -> Theme {
    Theme { name: std::borrow::Cow::Owned(name.to_string()), ..th.clone() }
}

#[test]
fn a_copy_of_every_builtin_reads_back_as_that_theme() {
    let d = Dir::new("copy-all");
    assert_eq!(BUILTINS.len(), 12);
    for (n, look) in LOOKS.into_iter().enumerate() {
        for t in BUILTINS {
            let name = format!("{}-{n}", t.name);
            let path = copy(&d.0, &t.name, &name, look).unwrap();
            assert_eq!(path, theme_file::path(&d.0, &name));
            let back = load(&name, look, Some(&d.0)).unwrap();
            assert_eq!(back, named(&look.theme(&t.name), &name), "{} on {look:?}", t.name);
            let text = d.text(&name);
            assert!(text.contains(&format!("extends = \"{}\"\n", t.name)), "{text}");
        }
    }
    // A family and `auto` copy the theme drawn now, extending it.
    let look = LOOKS[1];
    copy(&d.0, "catppuccin", "cat", look).unwrap();
    assert!(d.text("cat").contains("extends = \"catppuccin-latte\"\n"));
    assert_eq!(load("cat", look, Some(&d.0)).unwrap(), named(&look.theme("catppuccin"), "cat"));
    copy(&d.0, "AUTO", "au", LOOKS[0]).unwrap();
    assert_eq!(load("au", LOOKS[0], Some(&d.0)).unwrap(), named(&LOOKS[0].theme("auto"), "au"));
}

#[test]
fn every_key_is_set_under_what_it_is_for() {
    let d = Dir::new("copy-keys");
    copy(&d.0, "dark", "mine", LOOKS[0]).unwrap();
    let text = d.text("mine");
    let lines: Vec<&str> = text.lines().collect();
    let colors_at = lines.iter().position(|l| *l == "[colors]").unwrap();
    let styles_at = lines.iter().position(|l| *l == "[styles]").unwrap();
    for (rows, from, to, all_set) in
        [(COLOR_ABOUT, colors_at, styles_at, true), (STYLE_ABOUT, styles_at, lines.len(), false)]
    {
        for (token, about) in rows {
            let at = (from..to)
                .find(|&i| {
                    lines[i].starts_with(&format!("{token} = ")) || lines[i].starts_with(&format!("# {token} = "))
                })
                .unwrap_or_else(|| panic!("{token} missing"));
            assert_eq!(lines[at - 1], format!("# {}", about.replace('`', "")), "{token}");
            if all_set {
                assert!(lines[at].starts_with(&format!("{token} = ")), "{token} is set");
            } else {
                // A built-in theme sets no style: each is its look now, commented out.
                assert!(lines[at].starts_with(&format!("# {token} = ")), "{token} is commented out");
            }
        }
    }
    assert!(text.contains("\nbg = \"#14161b\"\n") && text.contains("\ndim = { toward = \"#0b0c10\", keep = 55 }\n"));
    // A theme file's own style is set in its copy, and kept.
    d.put("styled", "extends = \"dark\"\n[styles]\nlink = { fg = \"cyan\", modifiers = [\"underlined\"] }\n");
    copy(&d.0, "styled", "styled-copy", LOOKS[0]).unwrap();
    assert!(d.text("styled-copy").contains("\nlink = { fg = \"cyan\", modifiers = [\"underlined\"] }\n"));
    let back = load("styled-copy", LOOKS[0], Some(&d.0)).unwrap();
    assert_eq!(back, named(&load("styled", LOOKS[0], Some(&d.0)).unwrap(), "styled-copy"));
}

#[test]
fn the_words_above_each_key_are_those_of_the_doc() {
    let doc = include_str!("../../../../../docs/themes.md");
    for (token, about) in COLOR_ABOUT.iter().chain(STYLE_ABOUT) {
        assert!(doc.contains(&format!("| `{token}` | {about} |")), "docs/themes.md says of {token}: {about}");
    }
    let names: Vec<&str> = COLOR_ABOUT.iter().map(|(t, _)| *t).collect();
    let tokens: Vec<&str> = theme_file::COLOR_TOKENS.iter().chain(theme_file::OTHER_COLOR_TOKENS).copied().collect();
    assert_eq!(names, tokens);
    let styles: Vec<&str> = STYLE_ABOUT.iter().map(|(t, _)| *t).collect();
    assert_eq!(styles, theme_file::STYLE_TOKENS);
}

#[test]
fn a_new_file_sets_nothing_and_reads_as_the_theme_it_extends() {
    let d = Dir::new("new");
    new(&d.0, "fresh", "nord", LOOKS[0]).unwrap();
    let text = d.text("fresh");
    assert!(text.contains("extends = \"nord\"\n"));
    let spec = theme_file::parse(&text).unwrap();
    assert_eq!(spec, ThemeSpecOnly::extends("nord", &text));
    for (token, _) in COLOR_ABOUT.iter().chain(STYLE_ABOUT) {
        assert!(text.contains(&format!("\n# {token} = ")), "{token} listed");
    }
    assert_eq!(load("fresh", LOOKS[0], Some(&d.0)).unwrap(), named(&LOOKS[0].theme("nord"), "fresh"));
    assert_eq!(update(&d.0, "fresh", LOOKS[0]), Ok(Updated::Complete { path: theme_file::path(&d.0, "fresh") }));
    assert!(matches!(new(&d.0, "x", "auto", LOOKS[0]), Err(AuthorError::Source(_))));
}

/// The spec of a file that sets only `extends`.
struct ThemeSpecOnly;

impl ThemeSpecOnly {
    fn extends(base: &str, text: &str) -> theme_file::ThemeSpec {
        let line = text.lines().position(|l| l.starts_with("extends")).unwrap() + 1;
        theme_file::ThemeSpec { extends: Some((base.to_string(), line)), ..Default::default() }
    }
}

#[test]
fn a_taken_name_is_refused_and_the_file_kept() {
    let d = Dir::new("exists");
    let path = d.put("taken", "# mine\n");
    assert_eq!(copy(&d.0, "dark", "taken", LOOKS[0]), Err(AuthorError::Exists(path.clone())));
    assert_eq!(new(&d.0, "taken", "dark", LOOKS[0]), Err(AuthorError::Exists(path)));
    assert_eq!(d.text("taken"), "# mine\n");
}

#[test]
fn bad_names_are_refused_before_any_path_is_made() {
    let d = Dir::new("names");
    for bad in ["../x", "a.b", "", "nul", "COM1", &"x".repeat(65)] {
        assert_eq!(copy(&d.0, "dark", bad, LOOKS[0]), Err(AuthorError::Name(NameError::Invalid(bad.to_string()))));
        assert_eq!(new(&d.0, bad, "dark", LOOKS[0]), Err(AuthorError::Name(NameError::Invalid(bad.to_string()))));
    }
    assert_eq!(copy(&d.0, "dark", "Nord", LOOKS[0]), Err(AuthorError::Name(NameError::Builtin("Nord".into()))));
    assert!(!d.0.exists(), "nothing made");
    assert!(matches!(copy(&d.0, "../x", "y", LOOKS[0]), Err(AuthorError::Source(ThemeError::BadName(_)))));
    assert!(matches!(copy(&d.0, "none", "y", LOOKS[0]), Err(AuthorError::Source(ThemeError::Unknown { .. }))));
}

#[cfg(unix)]
#[test]
fn the_themes_folder_is_made_private_and_the_files_too() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new("modes");
    copy(&d.0, "dark", "a", LOOKS[0]).unwrap();
    new(&d.0, "b", "dark", LOOKS[0]).unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&d.0), 0o700);
    assert_eq!(mode(&theme_file::path(&d.0, "a")), 0o600);
    assert_eq!(mode(&theme_file::path(&d.0, "b")), 0o600);
}

#[test]
fn the_first_free_copy_name() {
    let d = Dir::new("free");
    assert_eq!(free_copy_name(None, "dark"), "dark-copy");
    assert_eq!(free_copy_name(Some(&d.0), "dark"), "dark-copy");
    d.put("dark-copy", "");
    assert_eq!(free_copy_name(Some(&d.0), "dark"), "dark-copy-2");
    d.put("dark-copy-2", "");
    assert_eq!(free_copy_name(Some(&d.0), "dark"), "dark-copy-3");
}

/// `text` without the lines of `tokens` (each key and the comment above it).
fn without(text: &str, tokens: &[&str]) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        let key =
            |l: &str| tokens.iter().any(|t| l.starts_with(&format!("{t} = ")) || l.starts_with(&format!("# {t} = ")));
        if key(l) || lines.get(i + 1).is_some_and(|n| key(n)) {
            continue;
        }
        out.push_str(l);
    }
    out
}

/// `new` is `old` with lines added only.
fn only_added(old: &str, new: &str) {
    let mut rest = new.lines();
    for l in old.lines() {
        assert!(rest.any(|n| n == l), "line kept in order: {l}");
    }
}

#[test]
fn update_adds_only_what_the_file_does_not_list() {
    let d = Dir::new("update");
    copy(&d.0, "dark", "old", LOOKS[0]).unwrap();
    let old = without(&d.text("old"), &["fg_dim", "avatars", "hint"]);
    let path = d.put("old", &old);
    let before = load("old", LOOKS[0], Some(&d.0)).unwrap();
    assert_eq!(update(&d.0, "old", LOOKS[0]), Ok(Updated::Added { path: path.clone(), count: 3 }));
    let new = d.text("old");
    only_added(&old, &new);
    let colors_at = new.find("\n[colors]\n").unwrap();
    let styles_at = new.find("\n[styles]\n").unwrap();
    let at = |s: &str| new.find(s).unwrap_or_else(|| panic!("{s} in {new}"));
    assert!(colors_at < at("\n# fg_dim = \"#5c6577\"\n") && at("\n# fg_dim = ") < styles_at);
    assert!(at("\n# avatars = [") < styles_at && at("\n# hint = ") > styles_at);
    // The meaning is the same, and a second update has nothing to add.
    assert_eq!(load("old", LOOKS[0], Some(&d.0)).unwrap(), before);
    assert_eq!(update(&d.0, "old", LOOKS[0]), Ok(Updated::Complete { path }));
    // A short file gets the tables it lacks.
    d.put("short", "extends = \"dark\"\n");
    assert!(matches!(update(&d.0, "short", LOOKS[0]), Ok(Updated::Added { count: 38, .. })));
    let short = d.text("short");
    assert!(
        short.starts_with("extends = \"dark\"\n") && short.contains("\n[colors]\n") && short.contains("\n[styles]\n")
    );
}

#[test]
fn update_keeps_windows_line_breaks() {
    let d = Dir::new("update-crlf");
    copy(&d.0, "dark", "win", LOOKS[0]).unwrap();
    let old = without(&d.text("win"), &["fg_dim", "code"]).replace('\n', "\r\n");
    d.put("win", &old);
    assert!(matches!(update(&d.0, "win", LOOKS[0]), Ok(Updated::Added { count: 2, .. })));
    let new = d.text("win");
    assert!(!new.replace("\r\n", "").contains('\n'), "only CRLF line breaks");
    only_added(&old, &new);
}

#[test]
fn update_that_cannot_be_done_safely_leaves_the_file_as_it_is() {
    let d = Dir::new("update-refused");
    assert_eq!(update(&d.0, "Dark", LOOKS[0]), Err(AuthorError::NotAFile("Dark".into())));
    assert_eq!(update(&d.0, "../x", LOOKS[0]), Err(AuthorError::Source(ThemeError::BadName("../x".into()))));
    assert!(matches!(update(&d.0, "none", LOOKS[0]), Err(AuthorError::Source(ThemeError::Unknown { .. }))));
    let broken = "extends = \"dark\"\n[colors]\nbg = \"nope\"\n";
    d.put("broken", broken);
    assert!(matches!(
        update(&d.0, "broken", LOOKS[0]),
        Err(AuthorError::Source(ThemeError::File { line: Some(3), .. }))
    ));
    assert_eq!(d.text("broken"), broken);
    // Colours as dotted keys: a [colors] table added would not read.
    let dotted = "extends = \"dark\"\ncolors.bg = \"#000000\"\n";
    let path = d.put("dotted", dotted);
    assert_eq!(update(&d.0, "dotted", LOOKS[0]), Err(AuthorError::Changed(path)));
    assert_eq!(d.text("dotted"), dotted);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = d.put("locked", "extends = \"dark\"\n");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(update(&d.0, "locked", LOOKS[0]), Err(AuthorError::Write(path.clone())));
        assert_eq!(d.text("locked"), "extends = \"dark\"\n");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}

#[test]
fn update_refuses_a_file_changed_since_it_was_read() {
    let d = Dir::new("update-race");
    let path = d.put("moving", "extends = \"dark\"\n");
    // The file as it was read, then changed before the write.
    let (text, _) = theme_file::read_text(&d.0, "moving").unwrap();
    std::fs::write(&path, "extends = \"nord\"\n").unwrap();
    assert_eq!(update::write_checked(&path, &text, "new text"), Err(AuthorError::Changed(path.clone())));
    assert_eq!(d.text("moving"), "extends = \"nord\"\n");
}

#[cfg(unix)]
#[test]
fn update_keeps_a_private_file_private() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new("update-mode");
    copy(&d.0, "dark", "kept", LOOKS[0]).unwrap();
    let path = d.put("kept", &without(&d.text("kept"), &["fg"]));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(update(&d.0, "kept", LOOKS[0]), Ok(Updated::Added { count: 1, .. })));
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn update_never_waits_on_a_pipe_nor_writes_through_a_link() {
    let d = Dir::new("update-nofollow");
    let text = "extends = \"dark\"\n";
    // A pipe where the file was: refused at once, not waited on.
    let pipe = theme_file::path(&d.0, "pipe");
    std::fs::create_dir_all(&d.0).unwrap();
    assert!(std::process::Command::new("mkfifo").arg(&pipe).status().unwrap().success());
    let (tx, rx) = std::sync::mpsc::channel();
    let at = pipe.clone();
    std::thread::spawn(move || tx.send(update::write_checked(&at, text, "new text")).unwrap());
    let done = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("waited on a pipe");
    assert_eq!(done, Err(AuthorError::Write(pipe)));
    // A link put where the file was: refused, its target untouched.
    let victim = d.1.join("victim");
    std::fs::write(&victim, text).unwrap();
    let link = theme_file::path(&d.0, "link");
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    assert_eq!(update::write_checked(&link, text, "new text"), Err(AuthorError::Write(link.clone())));
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), text);
}
