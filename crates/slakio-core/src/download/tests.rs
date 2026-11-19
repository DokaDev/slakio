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
    for device in ["CONIN$", "conout$.txt", "COM\u{B9}", "com\u{B2}.log", "LPT\u{B3}.txt", "lpt9"] {
        assert_eq!(safe(device), format!("_{device}"), "{device}");
    }
    assert_eq!(safe("COM\u{B9}\u{B2}"), "COM\u{B9}\u{B2}", "one digit only");
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
    let path = s.finish().unwrap().path;
    assert_eq!(path, dir.join("a (2).txt"));
    assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"mine");
    assert_eq!(fs::read(&path).unwrap(), b"theirs");
    let mut s = Saving::start(&dir, &Remote::new("a.txt"), 100).unwrap();
    s.write(b"third").unwrap();
    assert_eq!(s.finish().unwrap().path, dir.join("a (3).txt"));
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
    assert_eq!(s.finish().unwrap().path, dir.join("a (2).txt"));
    assert_eq!(fs::read(&outside).unwrap(), b"keep");
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(outside.parent().unwrap());
}

#[test]
fn only_viewer_kinds_are_offered_to_open() {
    for ok in ["a.pdf", "b.PNG", "c.txt", "d.log", "e.mp4", "f.mp3", "g.jpeg"] {
        assert!(openable(Path::new(ok)), "{ok}");
    }
    for no in ["a.exe", "b.app", "c.sh", "d.command", "e.pkg", "f.dmg", "g.html", "h.svg", "i.doc", "j.docm", "k.zip"] {
        assert!(!openable(Path::new(no)), "{no}");
    }
    // Office documents (macros, embedded objects, external references), tables a spreadsheet
    // reads formulas from, and text an editor may run code for are only saved.
    for no in ["a.docx", "b.xlsx", "c.pptx", "d.csv", "e.tsv", "f.md", "g.json", "h.odt", "i.pages", "j.numbers"] {
        assert!(!openable(Path::new(no)), "{no}");
    }
    assert!(!openable(Path::new("noext")));
    assert!(!openable(Path::new("report.pdf.exe")));
}

#[cfg(unix)]
#[test]
fn a_folder_made_for_downloads_and_the_part_file_are_the_users_alone() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = temp_dir("private");
    let made = dir.join("made");
    let s = Saving::start(&made, &Remote::new("a.txt"), 100).unwrap();
    assert_eq!(mode(&made), 0o700, "a folder slakio makes");
    let part = made.join(&names(&made)[0]);
    assert_eq!(mode(&part), 0o600, "the part file");
    drop(s);
    // A folder that is there is left as it is.
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    let s = Saving::start(&dir, &Remote::new("b.txt"), 100).unwrap();
    assert_eq!(mode(&dir), 0o755);
    let saved = s.finish().unwrap().path;
    assert_eq!(mode(&saved), 0o600, "the file saved");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_file_is_marked_as_from_the_internet_before_it_has_its_name() {
    let dir = temp_dir("mark");
    let mut s = Saving::start(&dir, &Remote::new("a.pdf"), 100).unwrap();
    s.write(b"%PDF").unwrap();
    let done = s.finish().unwrap();
    assert!(!done.unmarked);
    #[cfg(target_os = "macos")]
    {
        let mut value = [0u8; 128];
        let n = rustix::fs::getxattr(&done.path, QUARANTINE, &mut value[..]).expect("the quarantine attribute");
        let value = String::from_utf8_lossy(&value[..n]).into_owned();
        assert!(value.starts_with("0081;") && value.ends_with(";slakio;"), "{value}");
    }
    #[cfg(windows)]
    {
        let mut stream = done.path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        assert_eq!(fs::read_to_string(stream).unwrap(), ZONE_IDENTIFIER);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn part_files_of_runs_that_are_gone_are_swept_and_nothing_else() {
    let dir = temp_dir("sweep");
    let me = std::process::id();
    // A process id no system hands out (above Linux's and macOS's most).
    let gone = 99_999_999u32;
    let files = [
        format!(".a.pdf.{gone}-1.part"),
        format!(".b.{me}-1.part"),
        "keep.part".to_string(),
        format!("c.{gone}-1.part"),
        format!(".d.{gone}-x.part"),
        "report.pdf".to_string(),
    ];
    for f in &files {
        fs::write(dir.join(f), b"x").unwrap();
    }
    let s = Saving::start(&dir, &Remote::new("e.txt"), 10).unwrap();
    sweep(&dir);
    let mut want: Vec<String> = files[1..].to_vec();
    if cfg!(not(any(target_os = "macos", target_os = "linux"))) {
        // Only the age tells there: a part file written just now stays.
        want.push(files[0].clone());
    }
    want.push(format!(".e.txt.{me}-1.part"));
    want.sort();
    assert_eq!(names(&dir), want, "this run's own part files and every other file stay");
    drop(s);
    sweep(Path::new("/nonexistent/slakio-sweep"));
    assert_eq!(part_pid(".x.y.12-3.part"), Some(12));
    assert_eq!(part_pid(".x.12-3.part.txt"), None);
    let _ = fs::remove_dir_all(&dir);
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
