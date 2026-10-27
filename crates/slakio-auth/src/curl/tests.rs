//! The parser against the shapes of every browser's copy, built from synthetic values (no
//! file holds a token-shaped string: the prefixes are joined at run time).

use super::*;
use proptest::prelude::*;

fn token(rest: &str) -> String {
    format!("{TOKEN_PREFIX}{rest}")
}

fn cookie(rest: &str) -> String {
    format!("{COOKIE_PREFIX}{rest}")
}

const URL: &str = "https://example-team.slack.com/api/client.counts?_x_id=abc-1.2&slack_route=T0TEST&fp=6e";

/// Chrome (macOS, bash): cookies in `-b`, a multipart body in `$'…'`.
fn chrome(t: &str, d: &str, ds: Option<&str>) -> String {
    let ds = ds.map(|v| format!("; d-s={v}")).unwrap_or_default();
    format!(
        "curl '{URL}' \\\n  -H 'accept: */*' \\\n  -H 'content-type: multipart/form-data; boundary=----WebKitFormBoundaryAB' \\\n  -b 'b=abc; x=1; d={d}{ds}; lc=17' \\\n  -H 'origin: https://app.slack.com' \\\n  --data-raw $'------WebKitFormBoundaryAB\\r\\nContent-Disposition: form-data; name=\"token\"\\r\\n\\r\\n{t}\\r\\n------WebKitFormBoundaryAB--\\r\\n'"
    )
}

/// Firefox: `-H 'Cookie: …'`, a form body.
fn firefox(t: &str, d: &str, ds: Option<&str>) -> String {
    let ds = ds.map(|v| format!("; d-s={v}")).unwrap_or_default();
    format!(
        "curl '{URL}' --compressed -X POST -H 'User-Agent: Mozilla/5.0' -H 'Origin: https://app.slack.com' -H 'Cookie: d={d}{ds}; b=1' --data-raw 'token={t}&_x_reason=x'"
    )
}

/// Safari: one header per line, `--data-binary`.
fn safari(t: &str, d: &str, ds: Option<&str>) -> String {
    let ds = ds.map(|v| format!("d-s={v}; ")).unwrap_or_default();
    format!(
        "curl '{URL}' \\\n-X 'POST' \\\n-H 'Cookie: {ds}d={d}' \\\n-H 'Origin: https://app.slack.com' \\\n--data-binary 'token={t}'"
    )
}

/// Windows `cmd`: everything in `^"…^"`, `%` and `&` escaped.
fn cmd(t: &str, d: &str, ds: Option<&str>) -> String {
    let ds = ds.map(|v| format!("; d-s={v}")).unwrap_or_default();
    let url = URL.replace('&', "^&");
    let d = d.replace('%', "^%");
    format!("curl ^\"{url}^\" ^\n  -H ^\"cookie: d={d}{ds}^\" ^\n  --data-raw ^\"token={t}^\"")
}

/// PowerShell: the cookies as `System.Net.Cookie` objects.
fn powershell(t: &str, d: &str, ds: Option<&str>) -> String {
    let ds = ds
        .map(|v| {
            format!("\n$session.Cookies.Add((New-Object System.Net.Cookie(\"d-s\", \"{v}\", \"/\", \".slack.com\")))")
        })
        .unwrap_or_default();
    format!(
        "$session = New-Object Microsoft.PowerShell.Commands.WebRequestSession\n$session.Cookies.Add((New-Object System.Net.Cookie(\"d\", \"{d}\", \"/\", \".slack.com\"))){ds}\nInvoke-WebRequest -UseBasicParsing -Uri \"{URL}\" -Method \"POST\" -WebSession $session -Body \"token={t}\""
    )
}

type Shape = fn(&str, &str, Option<&str>) -> String;
const SHAPES: &[(&str, Shape)] =
    &[("chrome", chrome), ("firefox", firefox), ("safari", safari), ("cmd", cmd), ("powershell", powershell)];

#[test]
fn every_browser_copy_gives_the_workspace_token_and_cookies() {
    let (t, d) = (token("1111-2222-3333-abcdef"), cookie("AbC%2FdEf%2BgHi%3D%3D"));
    for (name, shape) in SHAPES {
        for ds in [None, Some("1700000000")] {
            let p = parse(&shape(&t, &d, ds)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(p.host, "example-team.slack.com", "{name}");
            assert_eq!(p.credentials.token(), t, "{name}");
            assert_eq!(p.credentials.cookie(), d, "{name}");
            assert_eq!(p.credentials.cookie_ds(), ds, "{name}");
        }
    }
}

#[test]
fn a_decoded_cookie_is_sent_encoded_again() {
    let p = parse(&firefox(&token("1-2-3-abcdefgh"), &cookie("ab/cd+ef=="), None)).unwrap();
    assert_eq!(p.credentials.cookie(), cookie("ab%2Fcd%2Bef%3D%3D"));
}

#[test]
fn an_enterprise_workspace_address_is_one() {
    let text = firefox(&token("1-2-3-abcdefgh"), &cookie("abcdefghij"), None)
        .replace("example-team.slack.com", "Big-Org.Enterprise.slack.com");
    assert_eq!(parse(&text).unwrap().host, "big-org.enterprise.slack.com");
}

#[test]
fn missing_pieces_say_what_is_missing() {
    let (t, d) = (token("1-2-3-abcdefgh"), cookie("abcdefghij"));
    assert_eq!(parse(""), Err(PasteError::Empty));
    assert_eq!(parse("  \n\t "), Err(PasteError::Empty));
    assert_eq!(parse(&"a".repeat(PASTE_MAX + 1)), Err(PasteError::TooLong));
    let no_cookie = chrome(&t, &d, None).replace(&format!("d={d}"), "");
    assert_eq!(parse(&no_cookie), Err(PasteError::NoCookie));
    let no_token = chrome(&t, &d, None).replace(&t, "");
    assert_eq!(parse(&no_token), Err(PasteError::NoToken));
    let elsewhere = chrome(&t, &d, None).replace(URL, "https://example.com/api/x");
    assert_eq!(parse(&elsewhere.replace("https://app.slack.com", "https://example.org")), Err(PasteError::NoAddress));
    let app = chrome(&t, &d, None).replace("example-team.slack.com", "app.slack.com");
    assert_eq!(parse(&app), Err(PasteError::NotWorkspace));
    let edge = firefox(&t, &d, None).replace("example-team.slack.com", "edgeapi.slack.com");
    assert_eq!(parse(&edge), Err(PasteError::NotWorkspace));
    let two = format!("{} {}", chrome(&t, &d, None), token("9-9-9-zzzzzzzz"));
    assert_eq!(parse(&two), Err(PasteError::Ambiguous));
    // The same token twice is one sign-in.
    assert!(parse(&format!("{} {t}", chrome(&t, &d, None))).is_ok());
}

#[test]
fn a_bare_token_prefix_or_a_short_value_is_not_a_sign_in() {
    let text = firefox(TOKEN_PREFIX, &cookie("abcdefghij"), None);
    assert_eq!(parse(&text), Err(PasteError::NoToken));
}

#[test]
fn workspace_hosts() {
    for ok in ["a.slack.com", "my-team.slack.com", "x1.enterprise.slack.com"] {
        assert!(workspace_host(ok), "{ok}");
    }
    for no in [
        "slack.com",
        "enterprise.slack.com",
        "app.enterprise.slack.com",
        "wss-primary.slack.com",
        "downloads.slack.com",
        "app.slack.com",
        "edgeapi.slack.com",
        "-a.slack.com",
        "a.b.slack.com",
        "a.slack.com.evil",
        "A.slack.com",
    ] {
        assert!(!workspace_host(no), "{no}");
    }
}

#[test]
fn a_parsed_paste_never_prints_its_credentials() {
    let (t, d) = (token("1-2-3-abcdefgh"), cookie("abcdefghij"));
    let printed = format!("{:?}", parse(&chrome(&t, &d, Some("17"))).unwrap());
    assert!(!printed.contains(&t) && !printed.contains(&d), "{printed}");
    assert!(printed.contains("example-team.slack.com"), "{printed}");
}

proptest! {
    #[test]
    fn any_text_parses_or_fails_without_a_panic(text in ".{0,400}") {
        let _ = parse(&text);
    }

    #[test]
    fn values_come_back_exactly_and_errors_never_hold_them(
        t in "[0-9]{1,12}-[0-9]{1,12}-[A-Za-z0-9]{8,64}",
        d in "[A-Za-z0-9]{8,40}(%2F[A-Za-z0-9]{1,20}){0,3}(%3D){0,2}",
        ds in proptest::option::of("[0-9]{6,12}"),
        shape in 0..SHAPES.len(),
        cut in 0..3u8,
    ) {
        let (t, d) = (token(&t), cookie(&d));
        let mut text = SHAPES[shape].1(&t, &d, ds.as_deref());
        match cut {
            0 => {
                let p = parse(&text).unwrap();
                prop_assert_eq!(p.credentials.token(), t.as_str());
                prop_assert_eq!(p.credentials.cookie(), d.as_str());
                prop_assert_eq!(p.credentials.cookie_ds(), ds.as_deref());
                return Ok(());
            }
            1 => text = text.replace(&t, ""),
            _ => text = text.replace(&d, "").replace(&d.replace('%', "^%"), ""),
        }
        let e = parse(&text).unwrap_err();
        let said = format!("{e} {e:?}");
        prop_assert!(!said.contains(&t) && !said.contains(&d), "{}", said);
    }
}

/// The characters the value scans look at for `text` (both prefixes).
fn steps(text: &str) -> usize {
    let mut n = 0;
    let _ = values(text, TOKEN_PREFIX, is_token_char, &mut n);
    let _ = values(text, COOKIE_PREFIX, is_cookie_char, &mut n);
    n
}

#[test]
fn a_hostile_paste_is_read_in_one_pass() {
    // The prefixes over and over (every one a candidate whose value runs to the end), and
    // many short distinct candidates: the work stays a few steps per character.
    for text in [
        TOKEN_PREFIX.repeat(PASTE_MAX / TOKEN_PREFIX.len()),
        COOKIE_PREFIX.repeat(PASTE_MAX / COOKIE_PREFIX.len()),
        (0..20_000).map(|i| format!("{TOKEN_PREFIX}{i:08} ")).collect::<String>(),
    ] {
        let n = steps(&text);
        assert!(n <= 2 * text.len(), "{n} steps for {} characters", text.len());
        assert!(parse(&text).is_err());
    }
}

#[test]
fn many_candidates_are_ambiguous() {
    let text = (0..100).map(|i| format!("{} ", token(&format!("{i:08}")))).collect::<String>();
    let text = format!("{} {text}", firefox(&token("1-2-3-abcdefgh"), &cookie("abcdefghij"), None));
    assert_eq!(parse(&text), Err(PasteError::Ambiguous));
}
