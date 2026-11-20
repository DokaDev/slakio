use super::*;

#[test]
fn a_photo_is_asked_for_with_nothing_of_the_session() {
    let h = avatar_headers(Os::current());
    for (name, _) in &h {
        for secret in ["cookie", "authorization", "origin", "referer"] {
            assert!(!name.eq_ignore_ascii_case(secret), "{name}");
        }
    }
    assert!(h.iter().any(|(n, v)| *n == "Sec-Fetch-Dest" && v == "image"));
}

#[test]
fn a_copy_on_the_file_host_carries_the_cookie_never_the_token() {
    let creds = Credentials::new(concat!("xo", "xc-SECRET"), "cookie-d");
    let h = thumb_headers(&creds, Os::current());
    assert!(h.iter().any(|(n, v)| *n == "Cookie" && v.contains("cookie-d")));
    assert!(h.iter().all(|(_, v)| !v.contains("SECRET")), "never the token");
    assert!(h.iter().any(|(n, v)| *n == "Sec-Fetch-Dest" && v == "image"));
}
