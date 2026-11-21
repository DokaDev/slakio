use super::*;
use crate::i18n::{I18n, Lang};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-theme-file-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn err(text: &str) -> (Option<usize>, Problem) {
    parse(text).expect_err("an error")
}

/// The words of reading `text` as theme `mine`, in English.
fn words(text: &str) -> (PathBuf, String) {
    let dir = scratch(&format!("words-{}", text.len()));
    let path = dir.join("mine.toml");
    std::fs::write(&path, text).unwrap();
    let e = read(&dir, "mine").expect_err("an error");
    let out = e.message(&I18n::new(Lang::En)).to_string();
    let _ = std::fs::remove_dir_all(&dir);
    (path, out)
}

#[test]
fn colours_are_hex_ansi_names_or_default() {
    assert_eq!(ColorSpec::parse("#1e1E2e"), Some(ColorSpec::Rgb(0x1e, 0x1e, 0x2e)));
    assert_eq!(ColorSpec::parse("red"), Some(ColorSpec::Ansi(1)));
    assert_eq!(ColorSpec::parse("Bright-Black"), Some(ColorSpec::Ansi(8)));
    assert_eq!(ColorSpec::parse("DEFAULT"), Some(ColorSpec::Default));
    for (i, name) in ANSI_NAMES.iter().enumerate() {
        assert_eq!(ColorSpec::parse(name), Some(ColorSpec::Ansi(i as u8)), "{name}");
    }
    for bad in ["#123", "#12345g", "1e1e2e", "grey", "", "#1234567"] {
        assert_eq!(ColorSpec::parse(bad), None, "{bad}");
    }
}

#[test]
fn the_minimal_file_sets_nothing() {
    assert_eq!(parse("").unwrap(), ThemeSpec::default());
    assert_eq!(parse("# nothing\n[colors]\n[styles]\n").unwrap(), ThemeSpec::default());
    assert!(!ThemeSpec::default().uses_rgb());
}

#[test]
fn a_file_is_read_in_file_order_with_every_form() {
    let text = "extends = \"Tokyo-Night\"\n\n[colors]\nwarning = \"yellow\"\nbg = \"#000000\"\nfg = \"default\"\n\
                workspaces = [\"#7aa2f7\", \"red\"]\navatars = [\"cyan\"]\navatar_tint = 20\n\
                dim = { toward = \"#101010\", keep = 55 }\nselection = \"blue\"\n\n[styles]\n\
                selection = { bg = \"blue\", modifiers = [\"bold\", \"Reversed\"] }\nlink = {}\n";
    let spec = parse(text).unwrap();
    assert_eq!(spec.extends, Some(("tokyo-night".into(), 1)));
    assert_eq!(
        spec.colors,
        [
            ("warning", ColorSpec::Ansi(3)),
            ("bg", ColorSpec::Rgb(0, 0, 0)),
            ("fg", ColorSpec::Default),
            ("selection", ColorSpec::Ansi(4)),
        ]
    );
    assert_eq!(spec.workspaces, Some(vec![ColorSpec::Rgb(0x7a, 0xa2, 0xf7), ColorSpec::Ansi(1)]));
    assert_eq!(spec.avatars, Some(vec![ColorSpec::Ansi(6)]));
    assert_eq!(spec.avatar_tint, Some(20));
    assert_eq!(spec.dim, Some(DimSpec::Blend { toward: (0x10, 0x10, 0x10), keep: 55 }));
    assert_eq!(
        spec.styles,
        [
            (
                "selection",
                StyleSpec {
                    fg: None,
                    bg: Some(ColorSpec::Ansi(4)),
                    modifiers: vec![ModifierSpec::Bold, ModifierSpec::Reversed]
                }
            ),
            ("link", StyleSpec::default()),
        ]
    );
    assert!(spec.uses_rgb());
    assert!(!parse("[colors]\nfg = \"red\"\n").unwrap().uses_rgb());
    assert_eq!(parse("[colors]\ndim = \"Modifier\"\n").unwrap().dim, Some(DimSpec::Modifier));
}

#[test]
fn every_modifier_and_every_token_is_taken() {
    let names: Vec<String> = ModifierSpec::ALL.iter().map(|(n, _)| format!("\"{}\"", n.to_uppercase())).collect();
    let spec = parse(&format!("[styles]\nhint = {{ modifiers = [{}] }}\n", names.join(", "))).unwrap();
    let all: Vec<ModifierSpec> = ModifierSpec::ALL.iter().map(|(_, m)| *m).collect();
    assert_eq!(spec.styles, [("hint", StyleSpec { fg: None, bg: None, modifiers: all })]);
    let colors: String = COLOR_TOKENS.iter().map(|t| format!("{t} = \"red\"\n")).collect();
    let styles: String = STYLE_TOKENS.iter().map(|t| format!("{t} = {{ fg = \"red\" }}\n")).collect();
    let spec = parse(&format!("[colors]\n{colors}[styles]\n{styles}")).unwrap();
    assert_eq!(spec.colors.iter().map(|c| c.0).collect::<Vec<_>>(), COLOR_TOKENS);
    assert_eq!(spec.styles.iter().map(|s| s.0).collect::<Vec<_>>(), STYLE_TOKENS);
    for base in crate::config::THEMES.iter().filter(|n| **n != "auto") {
        assert!(parse(&format!("extends = \"{base}\"\n")).is_ok(), "{base}");
    }
}

#[test]
fn every_problem_has_its_line() {
    assert_eq!(err("[colors]\nfg = \"#fff\"\n"), (Some(2), Problem::BadColor("#fff".into())));
    assert_eq!(err("[colors]\nborder_x = \"red\"\n"), (Some(2), Problem::UnknownToken("border_x".into())));
    assert_eq!(err("[colors]\nlink = \"red\"\n"), (Some(2), Problem::NotAColor("link".into())));
    assert_eq!(err("[styles]\nfg = { fg = \"red\" }\n"), (Some(2), Problem::NotAStyle("fg".into())));
    assert_eq!(err("[styles]\ndim = { fg = \"red\" }\n"), (Some(2), Problem::NotAStyle("dim".into())));
    assert_eq!(err("[styles]\nlink = { modifiers = [\"blink\"] }\n"), (Some(2), Problem::BadModifier("blink".into())));
    assert_eq!(err("[styles]\n\nlink = { fg = \"nope\" }\n"), (Some(3), Problem::BadColor("nope".into())));
    assert_eq!(err("\nextends = \"auto\"\n"), (Some(2), Problem::UnknownBase("auto".into())));
    assert_eq!(err("extends = \"solarized\"\n"), (Some(1), Problem::UnknownBase("solarized".into())));
    assert_eq!(err("[colors]\nfg = 3\n"), (Some(2), Problem::BadColor("3".into())));
    assert_eq!(err("[colors]\nworkspaces = []\n"), (Some(2), Problem::BadPalette("workspaces".into())));
    let many = vec!["\"red\""; MAX_PALETTE + 1].join(", ");
    assert_eq!(err(&format!("[colors]\navatars = [{many}]\n")), (Some(2), Problem::BadPalette("avatars".into())));
    assert_eq!(err("[colors]\navatars = \"red\"\n"), (Some(2), Problem::BadPalette("avatars".into())));
    assert_eq!(err("[colors]\nworkspaces = [\"red\", \"x\"]\n"), (Some(2), Problem::BadColor("x".into())));
    assert_eq!(err("[colors]\navatar_tint = 101\n"), (Some(2), Problem::BadPercent("avatar_tint".into())));
    assert_eq!(err("[colors]\navatar_tint = -1\n"), (Some(2), Problem::BadPercent("avatar_tint".into())));
    for bad in ["\"blend\"", "{ toward = \"red\", keep = 50 }", "{ toward = \"#000000\", keep = 101 }", "{ keep = 5 }"]
    {
        assert_eq!(err(&format!("[colors]\ndim = {bad}\n")), (Some(2), Problem::BadDim), "{bad}");
    }
    let (line, p) = err("theme = \"x\"\n");
    assert_eq!(line, Some(1));
    assert!(matches!(p, Problem::Toml(m) if m.contains("theme")), "an unknown top-level key");
    let (line, p) = err("[styles]\nlink = { fg = \"red\", underline = true }\n");
    assert_eq!(line, Some(2));
    assert!(matches!(p, Problem::Toml(m) if m.contains("underline")));
    assert!(matches!(err("[colors\n"), (Some(1), Problem::Toml(_))));
}

#[test]
fn of_several_problems_the_first_in_the_file_wins() {
    assert_eq!(
        err("[colors]\nwarning = \"x\"\nbg = \"y\"\n"),
        (Some(2), Problem::BadColor("x".into())),
        "warning comes first although bg sorts first"
    );
    assert_eq!(
        err("[styles]\nzz = {}\n[colors]\nbg = \"y\"\n"),
        (Some(2), Problem::UnknownToken("zz".into())),
        "a problem under [styles] before one under [colors]"
    );
    assert_eq!(
        err("[colors]\nbg = \"y\"\n\n[styles]\nlink = { fg = \"q\" }\n"),
        (Some(2), Problem::BadColor("y".into()))
    );
    assert_eq!(
        err("[colors]\nextends = \"dark\"\n"),
        (Some(2), Problem::UnknownToken("extends".into())),
        "extends under [colors] is a token"
    );
    assert_eq!(err("[colors]\nmention = \"red\"\nextends = \"x\"\n"), (Some(2), Problem::NotAColor("mention".into())));
}

#[test]
fn a_problem_is_worded_with_its_path_and_line() {
    let cases = [
        ("[colors]\nborder_x = \"red\"\n", "2: border_x is not a theme token"),
        ("\n\n[colors]\nlink = \"red\"\n", "4: link is a style: it goes under [styles]"),
        ("[styles]\nbg = {}\n", "2: bg is not a style: it goes under [colors]"),
        ("[colors]\nfg = \"#fff\"\n", "2: #fff is not a colour (#rrggbb, an ANSI name or default)"),
        (
            "[styles]\nlink = { modifiers = [\"blink\"] }\n",
            "2: blink is not a modifier (bold, dim, italic, underlined, reversed or crossed-out)",
        ),
        ("extends = \"solarized\"\n", "1: extends names no built-in theme: solarized"),
        ("[colors]\navatars = []\n", "2: avatars needs a list of 1 to 16 colours"),
        ("[colors]\navatar_tint = 500\n", "2: avatar_tint needs a whole number from 0 to 100"),
        ("[colors]\ndim = 1\n", "2: dim needs \"modifier\" or a table of toward (#rrggbb) and keep (0 to 100)"),
    ];
    for (text, want) in cases {
        let (path, got) = words(text);
        assert_eq!(got, format!("{}:{want}", path.display()), "{text}");
    }
    let (path, got) = words("[colors\n");
    assert!(got.starts_with(&format!("{}:1: ", path.display())), "{got}");
    let (path, got) = words("theme = 1\n");
    assert!(got.starts_with(&format!("{}:1: ", path.display())) && got.contains("theme"), "{got}");
    let dir = scratch("not-text");
    std::fs::write(dir.join("mine.toml"), b"[colors]\nfg = \"\xff\"\n").unwrap();
    let got = read(&dir, "mine").unwrap_err().message(&I18n::new(Lang::En)).to_string();
    assert_eq!(got, format!("{}: not UTF-8 text", dir.join("mine.toml").display()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn names_are_plain_file_names() {
    for ok in ["mine", "my-theme_2", "A", &"x".repeat(64)] {
        assert!(valid_name(ok), "{ok}");
        assert_eq!(check_name(ok), Ok(()), "{ok}");
    }
    let hangul = "\u{d14c}\u{b9c8}";
    for bad in ["", "../x", "a.b", "a b", "x.toml", "a/b", "a\\b", "a\0b", hangul, &"x".repeat(65)] {
        assert!(!valid_name(bad), "{bad}");
        assert_eq!(check_name(bad), Err(NameError::Invalid(bad.to_string())), "{bad}");
    }
    for device in ["CON", "nul", "Com1", "lpt9", "aux", "prn"] {
        assert!(valid_name(device));
        assert_eq!(check_name(device), Err(NameError::Invalid(device.to_string())), "{device}");
    }
    for fine in ["com0", "com10", "console", "lpt"] {
        assert_eq!(check_name(fine), Ok(()), "{fine}");
    }
    for builtin in ["Dark", "dark", "AUTO", "tokyo-night", "Gruvbox-Light"] {
        assert!(shadows(builtin));
        assert_eq!(check_name(builtin), Err(NameError::Builtin(builtin.to_string())), "{builtin}");
    }
    let en = I18n::new(Lang::En);
    assert_eq!(
        NameError::Invalid("../x".into()).message(&en),
        "../x is not a theme file's name (1 to 64 letters, digits, - and _)"
    );
    assert_eq!(NameError::Builtin("Dark".into()).message(&en), "Dark is the name of a built-in theme");
}

#[test]
fn a_bad_name_never_reaches_the_file_system() {
    let dir = scratch("bad-name");
    std::fs::write(dir.join("x.toml"), "").unwrap();
    for bad in ["../x", "a.b", "", "x/../x"] {
        assert_eq!(read(&dir.join("sub"), bad), Err(ThemeError::BadName(bad.to_string())), "{bad}");
    }
    std::fs::write(dir.join("dark.toml"), "").unwrap();
    assert_eq!(read(&dir, "Dark"), Err(ThemeError::Shadows { path: dir.join("Dark.toml") }), "never read");
    assert_eq!(read(&dir, "gone"), Err(ThemeError::Unknown { name: "gone".into(), dir: Some(dir.clone()) }));
    let en = I18n::new(Lang::En);
    assert_eq!(
        read(&dir, "gone").unwrap_err().message(&en),
        format!("Unknown theme gone: there is no {}", dir.join("gone.toml").display()).as_str()
    );
    assert_eq!(read(&dir, "x"), Ok(ThemeSpec::default()));
    assert!(!dir.join("sub").exists(), "reading creates nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_over_64_kib_is_refused() {
    let dir = scratch("size");
    let full = format!("#{}\n", "x".repeat(MAX_BYTES as usize - 2));
    assert_eq!(full.len() as u64, MAX_BYTES);
    std::fs::write(dir.join("full.toml"), &full).unwrap();
    assert_eq!(read(&dir, "full"), Ok(ThemeSpec::default()), "64 KiB exactly is read");
    std::fs::write(dir.join("big.toml"), format!("{full}\n")).unwrap();
    let e = read(&dir, "big").unwrap_err();
    assert_eq!(e, ThemeError::TooLarge { path: dir.join("big.toml") });
    assert_eq!(
        e.message(&I18n::new(Lang::En)),
        format!("{} is larger than 64 KiB", dir.join("big.toml").display()).as_str()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_or_a_pipe_is_refused() {
    let dir = scratch("links");
    std::fs::write(dir.join("real.toml"), "[colors]\nfg = \"red\"\n").unwrap();
    std::os::unix::fs::symlink(dir.join("real.toml"), dir.join("link.toml")).unwrap();
    let e = read(&dir, "link").unwrap_err();
    assert_eq!(e, ThemeError::Symlink { path: dir.join("link.toml") });
    assert_eq!(
        e.message(&I18n::new(Lang::En)),
        format!("{} is a symbolic link", dir.join("link.toml").display()).as_str()
    );
    let ok = std::process::Command::new("mkfifo").arg(dir.join("pipe.toml")).status().unwrap();
    assert!(ok.success());
    assert_eq!(read(&dir, "pipe"), Err(ThemeError::NotPlain { path: dir.join("pipe.toml") }), "never waits on it");
    std::fs::create_dir(dir.join("folder.toml")).unwrap();
    assert_eq!(read(&dir, "folder"), Err(ThemeError::NotPlain { path: dir.join("folder.toml") }));
    // The themes folder itself may be a link (a dotfile manager's).
    let linked = scratch("linked-dir");
    let themes = linked.join("themes");
    std::os::unix::fs::symlink(&dir, &themes).unwrap();
    assert!(read(&themes, "real").is_ok());
    assert_eq!(names(&themes).names, ["real"]);
    let _ = std::fs::remove_dir_all(&linked);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_listing_is_sorted_plain_files_with_valid_names() {
    let dir = scratch("listing");
    for f in ["zeta.toml", "Alpha.toml", "mid_1.toml", "a.b.toml", "notes.txt", "x.TOML", ".toml", "dark.toml"] {
        std::fs::write(dir.join(f), "").unwrap();
    }
    std::fs::create_dir(dir.join("folder.toml")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("zeta.toml"), dir.join("linked.toml")).unwrap();
    let listing = names(&dir);
    assert_eq!(listing.names, ["Alpha", "mid_1", "zeta"]);
    assert_eq!(listing.shadows, [dir.join("dark.toml")]);
    assert_eq!(names(&dir.join("missing")), Listing::default());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_listing_stops_at_256() {
    let dir = scratch("many");
    for i in 0..MAX_FILES + 10 {
        std::fs::write(dir.join(format!("t{i:04}.toml")), "").unwrap();
    }
    let listing = names(&dir);
    assert_eq!(listing.names.len(), MAX_FILES);
    assert_eq!(listing.names.first().map(String::as_str), Some("t0000"));
    assert_eq!(listing.names.last().map(String::as_str), Some("t0255"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_theme_folder_is_next_to_the_config_file() {
    assert_eq!(dir(Path::new("/c/slakio/config.toml")), Some(PathBuf::from("/c/slakio/themes")));
    assert_eq!(path(Path::new("/c/themes"), "mine"), PathBuf::from("/c/themes/mine.toml"));
}
