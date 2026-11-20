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
