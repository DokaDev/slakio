use super::*;

#[test]
fn a_thumbnail_comes_from_the_file_host_only() {
    let ok = format!("https://{FILE_HOST}/files-tmb/T1-F1/a_360.png");
    assert_eq!(thumb_address(&ok).as_deref(), Some(ok.as_str()));
    for bad in [
        format!("http://{FILE_HOST}/files-tmb/x.png"),
        format!("https://{FILE_HOST}.evil.example/x.png"),
        format!("https://{FILE_HOST}@evil.example/x.png"),
        format!("https://{FILE_HOST}:8443/x.png"),
        format!("https://{FILE_HOST}/a b.png"),
        format!("https://{FILE_HOST}/a.png#frag"),
        format!("https://{FILE_HOST}/a\\b.png"),
        format!("https://{FILE_HOST}"),
        format!("https://{}/x.png", AVATAR_HOSTS[0]),
        format!("https://{FILE_HOST}/{}", "a".repeat(2100)),
    ] {
        assert_eq!(thumb_address(&bad), None, "{bad}");
    }
}

#[test]
fn a_photo_comes_from_an_avatar_host_only() {
    for host in AVATAR_HOSTS {
        let ok = format!("https://{host}/2020-01-01/123_abc_72.jpg");
        assert_eq!(avatar_address(&ok).as_deref(), Some(ok.as_str()));
        assert_eq!(host_of(&ok), Some(host));
    }
    let gravatar = "https://secure.gravatar.com/avatar/abc.jpg?s=72&d=mm";
    assert!(avatar_address(gravatar).is_some());
    for bad in [
        format!("http://{}/x.png", AVATAR_HOSTS[0]),
        format!("https://a{}/x.png", AVATAR_HOSTS[0]),
        format!("https://{}.evil.example/x.png", AVATAR_HOSTS[0]),
        format!("https://{}@evil.example/x.png", AVATAR_HOSTS[1]),
        format!("https://{}:443/x.png", AVATAR_HOSTS[1]),
        "https://www.gravatar.com/avatar/x.png".to_string(),
        "https://evil.example/x.png".to_string(),
        format!("https://{FILE_HOST}/files-pri/x.png"),
        "javascript:alert(1)".to_string(),
        String::new(),
    ] {
        assert_eq!(avatar_address(&bad), None, "{bad}");
    }
}

#[test]
fn a_source_is_checked_again_by_its_kind() {
    let thumb = format!("https://{FILE_HOST}/files-tmb/x.png");
    let photo = format!("https://{}/x.png", AVATAR_HOSTS[0]);
    assert!(Source::Thumb(thumb.clone()).checked().is_some());
    assert!(Source::Avatar(photo.clone()).checked().is_some());
    // A thumbnail's address passed off as a photo (or the other way round) is refused: the
    // cookie goes to the file host only.
    assert!(Source::Avatar(thumb).checked().is_none());
    assert!(Source::Thumb(photo).checked().is_none());
}

#[test]
fn only_png_jpeg_and_gif_are_previewed() {
    for t in ["png", "jpg", "JPEG", "gif"] {
        assert!(previewed(t), "{t}");
    }
    for t in ["webp", "heic", "svg", "pdf", "binary", ""] {
        assert!(!previewed(t), "{t}");
    }
}

#[test]
fn slack_s_default_avatars_come_from_its_static_host_exactly() {
    let host = AVATAR_HOSTS[2];
    let ok = format!("https://{host}/df10d/img/avatars/ava_0010-72.png");
    assert_eq!(avatar_address(&ok).as_deref(), Some(ok.as_str()));
    assert_eq!(host_of(&ok), Some(host));
    for bad in [
        format!("http://{host}/img/avatars/ava_0010-72.png"),
        format!("https://{host}.evil.com/img/avatars/ava_0010-72.png"),
        format!("https://evil@{host}/img/avatars/ava_0010-72.png"),
        format!("https://{host}@evil.example/img/avatars/ava_0010-72.png"),
        format!("https://{host}:443/img/avatars/ava_0010-72.png"),
        format!("https://{host}:8443/img/avatars/ava_0010-72.png"),
        format!("https://x{host}/img/avatars/ava_0010-72.png"),
        format!("https://{host}"),
        format!("https://{host}/a.png#frag"),
    ] {
        assert_eq!(avatar_address(&bad), None, "{bad}");
    }
    // Its own default avatar is no file host copy.
    assert_eq!(thumb_address(&ok), None);
}

#[test]
fn a_gravatar_address_is_kept_only_when_its_fallback_is_slack_s() {
    let slack = AVATAR_HOSTS[2];
    let hash = "https://secure.gravatar.com/avatar/04939292775a6f4a1817e5e846c11609.jpg";
    let ok = [
        format!("{hash}?s=72&d=https%3A%2F%2F{slack}%2Fdf10d%2Fimg%2Favatars%2Fava_0010-72.png"),
        format!("{hash}?d=https%3A%2F%2F{}%2F2020-01-01%2F1_72.jpg&s=72", AVATAR_HOSTS[0]),
        format!("{hash}?s=72&d=https://{slack}/img/avatars/ava_0010-72.png"),
        format!("{hash}?s=72&d=mp"),
        format!("{hash}?d=404"),
        format!("{hash}?s=72"),
        hash.to_string(),
    ];
    for url in &ok {
        assert_eq!(avatar_address(url).as_deref(), Some(url.as_str()), "{url}");
    }
    for bad in [
        format!("{hash}?s=72&d=https%3A%2F%2Fevil.example%2Fa.png"),
        format!("{hash}?s=72&d=https://evil.example/a.png"),
        format!("{hash}?s=72&default=https%3A%2F%2Fevil.example%2Fa.png"),
        format!("{hash}?s=72&D=https%3A%2F%2Fevil.example%2Fa.png"),
        format!("{hash}?s=72&%64=https%3A%2F%2Fevil.example%2Fa.png"),
        format!("{hash}?d=mp&d=https%3A%2F%2Fevil.example%2Fa.png"),
        format!("{hash}?d=http%3A%2F%2F{slack}%2Fimg%2Fa.png"),
        format!("{hash}?d=https%3A%2F%2F{slack}.evil.com%2Fimg%2Fa.png"),
        format!("{hash}?d=https%3A%2F%2Fevil%40{slack}%2Fimg%2Fa.png"),
        format!("{hash}?d=https%3A%2F%2F{slack}%3A8443%2Fimg%2Fa.png"),
        format!("{hash}?d=https%3A%2F%2Fsecure.gravatar.com%2Favatar%2Fx.png"),
        format!("{hash}?d=%2F%2Fevil.example%2Fa.png"),
        format!("{hash}?d=%2Fimg%2Fa.png"),
        format!("{hash}?d=%zz"),
        format!("{hash}?d=https%3A%2F%2F{slack}%2Fimg%2Fa.png%"),
    ] {
        assert_eq!(avatar_address(&bad), None, "{bad}");
    }
}

#[test]
fn a_gravatar_address_names_only_gravatar_s_parameters_plainly() {
    let hash = "https://secure.gravatar.com/avatar/04939292775a6f4a1817e5e846c11609.jpg";
    let evil = "https%3A%2F%2Fevil.example%2Fa.png";
    for ok in [
        format!("{hash}?s=72&d=mp&r=g&f=y"),
        format!("{hash}?SIZE=72&Default=identicon&rating=pg&forcedefault=n"),
        format!("{hash}?s=72&&d=mp"),
    ] {
        assert_eq!(avatar_address(&ok).as_deref(), Some(ok.as_str()), "{ok}");
    }
    for bad in [
        // `;` anywhere in the query, as a separator or not.
        format!("{hash}?s=72;d={evil}"),
        format!("{hash}?s=72&d=mp;"),
        format!("{hash}?s=7;2"),
        // A name another server may trim or cut short.
        format!("{hash}?+d={evil}"),
        format!("{hash}?%20d={evil}"),
        format!("{hash}?d%20={evil}"),
        format!("{hash}?%09d={evil}"),
        format!("{hash}?d%00={evil}"),
        format!("{hash}?%00d={evil}"),
        // A name Gravatar does not have, or an array of one.
        format!("{hash}?d[]={evil}"),
        format!("{hash}?x=1"),
        format!("{hash}?s=72&callback=y"),
        // A broken escape in a name.
        format!("{hash}?%+64={evil}"),
        format!("{hash}?d=%+F"),
    ] {
        assert_eq!(avatar_address(&bad), None, "{bad}");
    }
}

#[test]
fn a_gravatar_address_gives_its_slack_fallback_and_nothing_else() {
    let slack = AVATAR_HOSTS[2];
    let hash = "https://secure.gravatar.com/avatar/04939292775a6f4a1817e5e846c11609.jpg";
    let fallback = format!("https://{slack}/df10d/img/avatars/ava_0010-72.png");
    let encoded = format!("{hash}?s=72&d=https%3A%2F%2F{slack}%2Fdf10d%2Fimg%2Favatars%2Fava_0010-72.png");
    assert_eq!(gravatar_fallback(&encoded).as_deref(), Some(fallback.as_str()));
    assert_eq!(
        gravatar_fallback(&format!("{hash}?s=72&d=https://{slack}/a.png")),
        Some(format!("https://{slack}/a.png"))
    );
    for none in [
        format!("{hash}?s=72&d=mp"),
        format!("{hash}?d=404"),
        format!("{hash}?s=72"),
        format!("{hash}?s=72&d=https%3A%2F%2Fi0.wp.com%2F{slack}%2Fa.png"),
        format!("{hash}?d=https%3A%2F%2Fsecure.gravatar.com%2Favatar%2Fx.png"),
        format!("https://{slack}/img/a.png?d=https%3A%2F%2F{slack}%2Fb.png"),
        format!("https://i0.wp.com/avatar/x.png?d=https%3A%2F%2F{slack}%2Fb.png"),
    ] {
        assert_eq!(gravatar_fallback(&none), None, "{none}");
    }
}

#[test]
fn an_emoji_address_of_another_script_is_kept_percent_encoded_on_the_same_host_only() {
    let host = EMOJI_HOSTS[0];
    let raw = format!("https://{host}/T1/\u{AC10}\u{C0AC}/ab.png");
    assert_eq!(emoji_address(&raw), Some(format!("https://{host}/T1/%EA%B0%90%EC%82%AC/ab.png")));
    let plain = format!("https://{host}/T1/party/ab.png");
    assert_eq!(emoji_address(&plain).as_deref(), Some(plain.as_str()), "ASCII as it is");
    for bad in [
        format!("https://{host}\u{2024}evil.example/T1/a.png"),
        format!("https://\u{0435}{}/T1/a.png", &host[1..]),
        format!("https://{host}.evil.example/T1/\u{AC10}.png"),
        format!("https://evil.example/{host}/\u{AC10}.png"),
        format!("https://{host}/T1/\u{AC10} b.png"),
        format!("https://{host}/T1/\u{AC10}#x.png"),
    ] {
        assert_eq!(emoji_address(&bad), None, "{bad}");
    }
}

#[test]
fn an_emoji_address_too_long_is_refused_before_it_is_encoded() {
    let host = EMOJI_HOSTS[0];
    let huge = format!("https://{host}/T1/{}/a.png", "\u{C548}".repeat(350_000));
    assert!(huge.len() > 1_000_000);
    assert_eq!(emoji_address(&huge), None);
    // Within the cap before encoding, over it after: refused too (by the encoded length).
    let near = format!("https://{host}/T1/{}/a.png", "\u{C548}".repeat(600));
    assert!(near.len() <= ADDRESS_MAX);
    assert_eq!(emoji_address(&near), None);
    let fits = format!("https://{host}/T1/{}/a.png", "\u{C548}".repeat(200));
    assert!(emoji_address(&fits).is_some_and(|a| a.len() <= ADDRESS_MAX));
}

#[test]
fn an_original_photo_becomes_its_72_pixel_copy_on_the_avatar_host() {
    let avatars = AVATAR_HOSTS[0];
    let sized = format!("https://{avatars}/2026-01-05/1234567_abcdef0123_72.jpg");
    for original in [
        "https://s3-us-west-2.amazonaws.com/slack-files2/avatars/2026-01-05/1234567_abcdef0123_original.jpg"
            .to_string(),
        format!("https://{avatars}/2026-01-05/1234567_abcdef0123_original.jpg"),
    ] {
        assert_eq!(avatar_sized(&original).as_deref(), Some(sized.as_str()), "{original}");
    }
    let png = "https://s3-eu-west-1.amazonaws.com/slack-files2/avatars/2025-12-31/T1-U2_original.PNG";
    assert_eq!(avatar_sized(png), Some(format!("https://{avatars}/2025-12-31/T1-U2_72.PNG")), "the extension kept");
}

#[test]
fn an_original_photo_on_a_lookalike_host_or_an_odd_path_is_no_photo() {
    let avatars = AVATAR_HOSTS[0];
    let tail = "2026-01-05/1234_abc_original.jpg";
    for bad in [
        // Lookalike hosts.
        format!("https://s3-us-west-2.amazonaws.com.evil.example/slack-files2/avatars/{tail}"),
        format!("https://s3-us-west-2.amazonaws.com@evil.invalid/slack-files2/avatars/{tail}"),
        format!("https://evil.example/s3-us-west-2.amazonaws.com/slack-files2/avatars/{tail}"),
        format!("https://s3-.amazonaws.com/slack-files2/avatars/{tail}"),
        format!("https://s3-us.west.amazonaws.com/slack-files2/avatars/{tail}"),
        format!("https://S3-US-WEST-2.amazonaws.com/slack-files2/avatars/{tail}"),
        format!("https://s3-us-west-2.amazonaws.com:443/slack-files2/avatars/{tail}"),
        format!("https://evils3-us-west-2.amazonaws.com/slack-files2/avatars/{tail}"),
        format!("https://x{avatars}/{tail}"),
        format!("https://{avatars}.evil.example/{tail}"),
        format!("https://{}/{tail}", AVATAR_HOSTS[1]),
        format!("https://{}/{tail}", AVATAR_HOSTS[2]),
        format!("http://{avatars}/{tail}"),
        // Another bucket or folder.
        format!("https://s3-us-west-2.amazonaws.com/other-bucket/avatars/{tail}"),
        format!("https://s3-us-west-2.amazonaws.com/slack-files2/{tail}"),
        // Paths that climb, nest or smuggle.
        format!("https://{avatars}/../{tail}"),
        format!("https://{avatars}/2026-01-05/../1234_abc_original.jpg"),
        format!("https://{avatars}/2026-01-05/a/1234_abc_original.jpg"),
        format!("https://{avatars}/2026-01-05/..%2f1234_abc_original.jpg"),
        format!("https://{avatars}/2026-01-05/1234.abc_original.jpg"),
        format!("https://{avatars}/{tail}?x=1"),
        format!("https://{avatars}/2026-01-05/1234_abc_original.jpg?.png"),
        format!("https://{avatars}/{tail}#frag"),
        format!("https://{avatars}/2026-01-05/1234_abc_original.jp%67"),
        format!("https://{avatars}/2026-01-05/1234_abc_original."),
        format!("https://{avatars}/2026-01-05/1234_abc_original"),
        format!("https://{avatars}/2026-01-05/_original.jpg"),
        format!("https://{avatars}/2026-01-05/1234_abc_72.jpg"),
        format!("https://{avatars}/2026-1-05/1234_abc_original.jpg"),
        format!("https://{avatars}/2026-01-0x/1234_abc_original.jpg"),
        format!("https://{avatars}/20260105xx/1234_abc_original.jpg"),
        format!("https://{avatars}/2026-01-05/1234 abc_original.jpg"),
        format!("https://{avatars}/2026-01-05/{}_original.jpg", "a".repeat(2100)),
        String::new(),
    ] {
        assert_eq!(avatar_sized(&bad), None, "{bad}");
    }
}

#[test]
fn a_guessed_72_pixel_photo_falls_back_to_its_other_extension_then_the_original() {
    let avatars = AVATAR_HOSTS[0];
    assert_eq!(
        avatar_fallbacks(&format!("https://{avatars}/2026-01-05/T1-U2_72.png")),
        [
            format!("https://{avatars}/2026-01-05/T1-U2_72.jpg"),
            format!("https://{avatars}/2026-01-05/T1-U2_original.png")
        ]
    );
    assert_eq!(
        avatar_fallbacks(&format!("https://{avatars}/2026-01-05/T1-U2_72.jpg")),
        [
            format!("https://{avatars}/2026-01-05/T1-U2_72.png"),
            format!("https://{avatars}/2026-01-05/T1-U2_original.jpg")
        ]
    );
}

/// A guard: a fallback is only ever another address on Slack's avatar host, of the same file.
#[test]
fn fallbacks_never_leave_the_avatar_host() {
    let (avatars, gravatar, statics) = (AVATAR_HOSTS[0], AVATAR_HOSTS[1], AVATAR_HOSTS[2]);
    for url in [
        format!("https://{gravatar}/avatar/abc_72.png"),
        format!("https://{statics}/2026-01-05/T1-U2_72.png"),
        format!("https://{avatars}.evil.example/2026-01-05/T1-U2_72.png"),
        format!("https://{avatars}:8443/2026-01-05/T1-U2_72.png"),
        format!("https://user@{avatars}/2026-01-05/T1-U2_72.png"),
        format!("http://{avatars}/2026-01-05/T1-U2_72.png"),
        format!("https://{avatars}/2026-01-05/T1-U2_48.png"),
        format!("https://{avatars}/2026-01-05/T1-U2_72.png?x=1"),
        format!("https://{avatars}/2026-01-05/../T1-U2_72.png"),
        format!("https://{avatars}/a/b/T1-U2_72.png"),
        format!("https://{avatars}/2026-01-05/T1%2FU2_72.png"),
        "https://s3-us-west-2.amazonaws.com/slack-files2/avatars/2026-01-05/T1-U2_72.png".to_string(),
    ] {
        assert_eq!(avatar_fallbacks(&url), Vec::<String>::new(), "{url}");
    }
    for fallback in avatar_fallbacks(&format!("https://{avatars}/2026-01-05/T1-U2_72.png")) {
        assert_eq!(avatar_address(&fallback).as_deref(), Some(fallback.as_str()));
        assert_eq!(host_of(&fallback), Some(avatars));
    }
}
