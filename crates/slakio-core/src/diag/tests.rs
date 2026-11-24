use super::*;
use crate::images::{AVATAR_HOSTS, EMOJI_HOSTS, Source};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-diag-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn off_unless_the_switch_is_one_and_then_nothing_is_written() {
    let dir = scratch("off");
    let path = dir.join("state").join("pictures.log");
    for value in [None, Some(""), Some("0"), Some("yes"), Some("true")] {
        let d = Diag::from_env(|k| (k == DEBUG_ENV).then(|| value.map(str::to_string)).flatten(), Some(path.clone()));
        assert!(!d.on(), "{value:?}");
        d.line(Kind::Avatar, Stage::Asked, "ok", &[("host", "x")]);
        d.flush();
        assert_eq!(d.id("abc"), "00000000");
        assert!(!d.first(Kind::Avatar, Stage::Drawn, "abc"));
    }
    assert!(!dir.exists(), "no file and no folder made");
    // On, but nowhere to write: off.
    assert!(!Diag::from_env(|_| Some("1".into()), None).on());
    assert!(Diag::from_env(|_| Some(" 1 ".into()), Some(path)).on());
}

#[test]
fn a_line_is_time_kind_stage_result_then_plain_pairs() {
    let dir = scratch("line");
    let path = dir.join("pictures.log");
    let d = Diag::to(path.clone());
    d.line(Kind::Avatar, Stage::Redirected, "refused", &[("host", "i0.wp.com"), ("hop", "1"), ("odd", "a b\"c\n")]);
    d.line(Kind::EmojiList, Stage::Api, "ok", &[("token", concat!("xo", "xc-123-456"))]);
    d.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    let (ts, rest) = lines[0].split_once(' ').unwrap();
    assert_eq!(ts.len(), "2026-01-05T09:00:00.000Z".len(), "{ts}");
    assert!(ts.ends_with('Z') && ts.as_bytes()[10] == b'T', "{ts}");
    assert_eq!(rest, "avatar redirected refused host=i0.wp.com hop=1 odd=a?b?c?");
    assert!(lines[1].ends_with("emoji-list api ok [secret]"), "{}", lines[1]);
    assert!(!text.contains("xoxc"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_id_is_eight_hex_digits_the_same_within_a_run_never_the_address() {
    let d = Diag::to(scratch("id").join("pictures.log"));
    let url = "https://avatars.example/U123/abc.png";
    let id = d.id(url);
    assert_eq!(id.len(), 8);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()), "{id}");
    assert_eq!(d.id(url), id);
    assert_eq!(d.clone().id(url), id, "every clone has the run's key");
    assert_ne!(d.id("https://avatars.example/U124/abc.png"), id);
    assert_ne!(Diag::to(scratch("id2").join("p.log")).id(url), id, "another run, another key");
    assert!(d.first(Kind::Avatar, Stage::Drawn, url));
    assert!(!d.first(Kind::Avatar, Stage::Drawn, url));
    assert!(d.first(Kind::Emoji, Stage::Drawn, url));
}

#[test]
fn the_file_is_private_and_rotated_once_at_the_cap() {
    let dir = scratch("rot");
    let path = dir.join("state").join("pictures.log");
    let d = Diag::to(path.clone());
    // Short of a secret's length, each.
    let filler = "a".repeat(30);
    // Past the cap: about 180 bytes a line.
    for n in 0..(LOG_MAX / 100) {
        let pairs = [("n", n.to_string()), ("a", filler.clone()), ("b", filler.clone()), ("c", filler.clone())];
        let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        d.line(Kind::Thumb, Stage::Status, "200", &pairs);
        if n % 256 == 0 {
            d.flush();
        }
    }
    d.flush();
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let old = dir.join("state").join("pictures.log.1");
    assert!(size(&path) > 0 && size(&path) <= LOG_MAX, "{}", size(&path));
    assert!(size(&old) > LOG_MAX / 2 && size(&old) <= LOG_MAX, "{}", size(&old));
    assert!(!dir.join("state").join("pictures.log.2").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&old), 0o600);
        assert_eq!(mode(&dir.join("state")), 0o700);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_link_is_not_written_through() {
    let dir = scratch("link");
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("elsewhere");
    std::fs::write(&target, "").unwrap();
    let link = dir.join("pictures.log");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let d = Diag::to(link);
    d.line(Kind::Avatar, Stage::Asked, "ok", &[]);
    d.flush();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hosts_only_never_a_path_a_port_or_a_user() {
    assert_eq!(host("https://Secure.Gravatar.com/avatar/abc?d=x"), "secure.gravatar.com");
    assert_eq!(host(&format!("https://user:pw{}i0.wp.com:443/x/y.png?ssl=1", '@')), "i0.wp.com");
    assert_eq!(host("/relative/path"), "relative");
    assert_eq!(host("https://ex ample.com/x"), "odd");
    assert_eq!(host("https:///x"), "odd");
}

#[test]
fn why_an_address_is_refused_in_a_word() {
    let avatar = |u: &str| Source::Avatar(u.to_string());
    let g = AVATAR_HOSTS[1];
    let slack = AVATAR_HOSTS[2];
    assert_eq!(refusal(&avatar(&format!("https://{slack}/x.png"))), None);
    assert_eq!(refusal(&avatar(&format!("http://{slack}/x.png"))), Some("scheme"));
    assert_eq!(refusal(&avatar("https://elsewhere.example/x.png")), Some("host"));
    assert_eq!(refusal(&avatar(&format!("https://{slack}/x y.png"))), Some("charset"));
    assert_eq!(refusal(&avatar(&format!("https://{g}/avatar/a?s=72;d=mp"))), Some("gravatar-semicolon"));
    // An `&` escaped for HTML leaves a `;` in the query.
    assert_eq!(refusal(&avatar(&format!("https://{g}/avatar/a?s=72&amp;d=mp"))), Some("gravatar-semicolon"));
    assert_eq!(refusal(&avatar(&format!("https://{g}/avatar/a?s=72&x=1"))), Some("gravatar-param"));
    assert_eq!(refusal(&avatar(&format!("https://{g}/avatar/a?d=%zz"))), Some("gravatar-escape"));
    assert_eq!(
        refusal(&avatar(&format!("https://{g}/avatar/a?d=https%3A%2F%2Fevil.example%2Fx"))),
        Some("gravatar-d-host")
    );
    assert_eq!(refusal(&Source::Emoji(format!("https://{}/T1/a/b.png", EMOJI_HOSTS[0]))), None);
    assert_eq!(refusal(&Source::Emoji(format!("https://{slack}/a.png"))), Some("host"));
}

#[test]
fn a_gravatar_fallback_tells_its_host_and_whether_it_is_taken() {
    let g = AVATAR_HOSTS[1];
    let slack = AVATAR_HOSTS[2];
    let to_slack = format!("https://{g}/avatar/abc.jpg?s=72&d=https%3A%2F%2F{slack}%2Fimg%2Fava_0001-72.png");
    assert_eq!(gravatar_fallback(&to_slack), Some((slack.to_string(), true)));
    let elsewhere = format!("https://{g}/avatar/abc.jpg?d=https%3A%2F%2Fi0.wp.com%2Fx.png");
    assert_eq!(gravatar_fallback(&elsewhere), Some(("i0.wp.com".to_string(), false)));
    assert_eq!(gravatar_fallback(&format!("https://{g}/avatar/abc.jpg?d=mp")), Some(("keyword".to_string(), true)));
    assert_eq!(gravatar_fallback(&format!("https://{g}/avatar/abc.jpg?s=72")), None);
    assert_eq!(gravatar_fallback(&format!("https://{slack}/x.png?d=mp")), None, "not Gravatar");
}

#[test]
fn times_and_reasons_in_plain_words() {
    let at = UNIX_EPOCH + Duration::from_millis(1_767_603_600_123);
    assert_eq!(rfc3339(at), "2026-01-05T09:00:00.123Z");
    assert_eq!(rfc3339(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00.000Z");
    assert_eq!(slug("this workspace's picture share is reached"), "lane-picture");
    assert_eq!(slug("the pictures' share is reached"), "pictures");
    assert_eq!(slug("something else entirely"), "other");
    assert_eq!(value(""), "-");
    assert_eq!(value(&"x".repeat(100)).len(), VALUE_MAX);
}

/// The request governor's reasons, as it words them.
const REASONS: [(&str, &str); 7] = [
    ("the request cap of this run is reached", "run-cap"),
    ("the hourly request budget is reached", "hourly-budget"),
    ("the hourly background share is reached", "hourly-background"),
    ("the per-minute request cap is reached", "per-minute"),
    ("the pictures' share is reached", "pictures"),
    ("this workspace's background share is reached", "lane-background"),
    ("this workspace's picture share is reached", "lane-picture"),
];

#[test]
fn every_governor_reason_is_a_short_word_the_log_keeps() {
    let dir = std::env::temp_dir().join(format!("slakio-diag-slug-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("pictures.log");
    let diag = Diag::to(path.clone());
    for (why, word) in REASONS {
        assert_eq!(slug(why), word, "{why}");
        diag.line(Kind::Avatar, Stage::CapReached, &slug(why), &[("lane", "workspace")]);
    }
    diag.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("[secret]"), "{text}");
    for (_, word) in REASONS {
        assert!(text.contains(&format!("avatar cap-reached {word} lane=workspace")), "{word}: {text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
