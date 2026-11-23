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
