use super::*;
use proptest::prelude::*;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-dl-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

fn safe(name: &str) -> String {
    safe_name(&Remote::new(name))
}

#[test]
fn a_hostile_name_becomes_a_plain_file_name() {
    assert_eq!(safe("report.pdf"), "report.pdf");
    assert_eq!(safe("../../etc/passwd"), "passwd");
    assert_eq!(safe("..\\..\\Windows\\win.ini"), "win.ini");
    assert_eq!(safe("/absolute/path.txt"), "path.txt");
    assert_eq!(safe(".."), "file");
    assert_eq!(safe("."), "file");
    assert_eq!(safe(""), "file");
    assert_eq!(safe(".bashrc"), "bashrc", "never a hidden file");
    assert_eq!(safe("-rf"), "rf", "never read as an option");
    assert_eq!(safe("a:b*c?.txt"), "a_b_c_.txt");
    assert_eq!(safe("name. . "), "name");
    assert_eq!(safe("CON"), "_CON");
    assert_eq!(safe("nul.txt"), "_nul.txt");
    assert_eq!(safe("com1.log"), "_com1.log");
    assert_eq!(safe("com0.log"), "com0.log");
    assert_eq!(safe("evil\x1b]0;title\x07.txt"), "evil.txt", "escape sequences are removed");
    assert_eq!(safe("a\u{202E}fdp.exe"), "afdp.exe", "bidi overrides are removed");
    assert_eq!(safe("tab\there.txt"), "tab here.txt");
}

#[test]
fn a_long_name_keeps_its_extension() {
    let long = format!("{}.pdf", "x".repeat(400));
    let s = safe(&long);
    assert!(s.len() <= NAME_MAX);
    assert!(s.ends_with(".pdf"), "{s}");
    let hangul = format!("{}.txt", "\u{D55C}".repeat(100));
    let s = safe(&hangul);
    assert!(s.len() <= NAME_MAX && s.ends_with(".txt"), "{s}");
}

#[test]
fn the_names_tried_add_a_number_before_the_extension() {
    assert_eq!(nth_name("a.pdf", 1), "a.pdf");
    assert_eq!(nth_name("a.pdf", 2), "a (2).pdf");
    assert_eq!(nth_name("archive.tar.gz", 3), "archive.tar (3).gz");
    assert_eq!(nth_name("noext", 2), "noext (2)");
}

#[test]
fn a_saved_file_never_replaces_one_there() {
    let dir = temp_dir("free");
    fs::write(dir.join("a.txt"), b"mine").unwrap();
    let mut s = Saving::start(&dir, &Remote::new("a.txt"), 100).unwrap();
    s.write(b"theirs").unwrap();
    let path = s.finish().unwrap();
    assert_eq!(path, dir.join("a (2).txt"));
    assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"mine");
    assert_eq!(fs::read(&path).unwrap(), b"theirs");
    let mut s = Saving::start(&dir, &Remote::new("a.txt"), 100).unwrap();
    s.write(b"third").unwrap();
    assert_eq!(s.finish().unwrap(), dir.join("a (3).txt"));
    assert_eq!(names(&dir), ["a (2).txt", "a (3).txt", "a.txt"], "no part file is left");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn past_the_cap_nothing_more_is_written_and_nothing_is_left() {
    let dir = temp_dir("cap");
    let mut s = Saving::start(&dir.join("new"), &Remote::new("big.bin"), 10).unwrap();
    s.write(b"12345").unwrap();
    assert!(matches!(s.write(b"123456"), Err(Unsaved::TooLarge)));
    assert_eq!(s.written(), 5);
    drop(s);
    assert!(names(&dir.join("new")).is_empty(), "the part file is removed");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_link_in_the_way_is_never_followed() {
    let dir = temp_dir("link");
    let outside = temp_dir("link-outside").join("target.txt");
    fs::write(&outside, b"keep").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("a.txt")).unwrap();
    let mut s = Saving::start(&dir, &Remote::new("a.txt"), 100).unwrap();
    s.write(b"new").unwrap();
    assert_eq!(s.finish().unwrap(), dir.join("a (2).txt"));
    assert_eq!(fs::read(&outside).unwrap(), b"keep");
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(outside.parent().unwrap());
}

#[test]
fn only_viewer_kinds_are_offered_to_open() {
    for ok in ["a.pdf", "b.PNG", "c.txt", "d.docx", "e.mp4"] {
        assert!(openable(Path::new(ok)), "{ok}");
    }
    for no in ["a.exe", "b.app", "c.sh", "d.command", "e.pkg", "f.dmg", "g.html", "h.svg", "i.doc", "j.docm", "k.zip"] {
        assert!(!openable(Path::new(no)), "{no}");
    }
    assert!(!openable(Path::new("noext")));
    assert!(!openable(Path::new("report.pdf.exe")));
}

proptest! {
    #[test]
    fn any_name_stays_in_the_folder(name in any::<String>()) {
        let s = safe(&name);
        prop_assert!(!s.is_empty());
        prop_assert!(s.len() <= NAME_MAX + 1, "{s}");
        prop_assert!(!s.contains(['/', '\\', '<', '>', ':', '"', '|', '?', '*']));
        prop_assert!(!s.chars().any(char::is_control));
        prop_assert!(!s.starts_with(['.', '-', ' ']));
        prop_assert!(!s.ends_with(['.', ' ']));
        prop_assert!(!reserved(&s));
        let dir = Path::new("/downloads");
        let joined = dir.join(&s);
        prop_assert_eq!(joined.parent(), Some(dir));
        prop_assert_eq!(joined.components().count(), dir.components().count() + 1);
    }

    #[test]
    fn path_like_names_stay_in_the_folder(parts in prop::collection::vec("[a-z.]{0,3}|\\.\\.|/|\\\\", 0..8)) {
        let s = safe(&parts.concat());
        let dir = Path::new("/downloads");
        let joined = dir.join(&s);
        prop_assert_eq!(joined.parent(), Some(dir));
        prop_assert!(s != "." && s != "..");
    }
}
