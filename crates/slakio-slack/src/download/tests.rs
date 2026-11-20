use super::*;

/// `rest` on the file host: `https://<FILE_HOST><rest>`.
fn at(rest: &str) -> String {
    format!("https://{FILE_HOST}{rest}")
}

#[test]
fn only_a_plain_address_on_the_file_host_is_downloaded() {
    let ok = at("/files-pri/T1-F1/download/report.pdf");
    assert_eq!(address(&ok), Some(ok.clone()));
    assert!(address(&at("/files-pri/T1-F1/download/a%20b.pdf?origin_team=T1")).is_some());
    let bad = [
        format!("http://{FILE_HOST}/x"),
        at(""),
        at(".evil.example/x"),
        at("@evil.test/x"),
        at(":8443/x"),
        at("/x").replace("https://files", "https://FILES"),
        format!("https://evil.example/{FILE_HOST}/x"),
        at("/a b"),
        at("/a#frag"),
        at("/a\\b"),
        at("/\u{e9}"),
        at("/a\r\nX: y"),
    ];
    for bad in bad {
        assert_eq!(address(&bad), None, "{bad}");
    }
    assert_eq!(address(&at(&format!("/{}", "a".repeat(ADDRESS_MAX)))), None);
}

#[test]
fn a_redirect_stays_on_the_file_host() {
    assert_eq!(redirect("/files-pri/x"), Some(at("/files-pri/x")));
    assert_eq!(redirect(&at("/y")), Some(at("/y")));
    let plain = format!("http://{FILE_HOST}/x");
    for bad in ["//evil.example/x", "https://evil.example/x", "files-pri/x", "", plain.as_str()] {
        assert_eq!(redirect(bad), None, "{bad}");
    }
}

#[test]
fn a_download_sends_the_cookie_and_never_the_token() {
    let c = Credentials::new(concat!("xo", "xc-SECRET"), "D").with_ds("S");
    let h = headers(&c, Os::Mac);
    let names: Vec<&str> = h.iter().map(|(n, _)| *n).collect();
    assert_eq!(names.iter().filter(|n| **n == "Cookie").count(), 1);
    assert!(h.iter().any(|(n, v)| *n == "Cookie" && v == "d=D; d-s=S"));
    assert!(!names.contains(&"Origin") && !names.contains(&"Authorization"));
    assert!(h.iter().all(|(_, v)| !v.contains("SECRET")));
    assert!(h.iter().any(|(n, v)| *n == "Sec-Fetch-Mode" && v == "navigate"));
}

#[test]
fn a_web_page_is_a_file_only_when_one_was_asked_for() {
    let file = |name: &str| File {
        id: slakio_core::model::FileId::new("F"),
        name: name.into(),
        kind: "".into(),
        size: None,
        download: None,
        thumb: None,
    };
    assert!(a_page_instead(Some("text/html; charset=utf-8"), &file("a.pdf")));
    assert!(a_page_instead(Some(" TEXT/HTML"), &file("a")));
    assert!(!a_page_instead(Some("text/html"), &file("page.HTML")));
    assert!(!a_page_instead(Some("application/pdf"), &file("a.pdf")));
    assert!(!a_page_instead(None, &file("a.pdf")));
}
