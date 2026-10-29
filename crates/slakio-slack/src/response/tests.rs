use super::*;
use slakio_core::backend::FailKind;

fn raw(status: u16, body: &str) -> Raw {
    Raw { status, retry_after: None, body: body.as_bytes().to_vec() }
}

#[derive(Debug, serde::Deserialize)]
struct Answer {
    n: Option<u32>,
}

#[test]
fn an_ok_answer_decodes_and_ignores_unknown_fields() {
    let a: Answer = decode(&raw(200, r#"{"ok":true,"n":3,"new_field":{"x":1}}"#)).unwrap();
    assert_eq!(a.n, Some(3));
    let a: Answer = decode(&raw(200, r#"{"ok":true}"#)).unwrap();
    assert_eq!(a.n, None, "missing is unknown, not zero");
}

#[test]
fn errors_map_to_the_cores_kinds() {
    let err = |status, body| decode::<Answer>(&raw(status, body)).unwrap_err();
    assert_eq!(err(200, r#"{"ok":false,"error":"invalid_auth"}"#).kind(), FailKind::Auth);
    assert_eq!(err(200, r#"{"ok":false,"error":"token_revoked"}"#).kind(), FailKind::Auth);
    assert_eq!(err(200, r#"{"ok":false,"error":"ratelimited"}"#).kind(), FailKind::RateLimited { retry_after: None });
    assert_eq!(
        err(200, r#"{"ok":false,"error":"channel_not_found"}"#).kind(),
        FailKind::Api { code: "channel_not_found".into() }
    );
    assert_eq!(err(200, r#"{"ok":false}"#).kind(), FailKind::Api { code: "no_error_code".into() });
    assert_eq!(err(200, "<html>hi</html>").kind(), FailKind::Network);
    assert_eq!(err(200, r#"{"n":1}"#).kind(), FailKind::Network, "no ok field: not Slack's answer");
    let e = err(503, "<html>secret page</html>");
    assert_eq!(e.kind(), FailKind::Network);
    assert_eq!(e.detail(), "HTTP 503", "an error page's body never reaches the log");
}

#[test]
fn a_429_carries_retry_after() {
    let r = Raw { status: 429, retry_after: Some(" 7 ".into()), body: Vec::new() };
    assert_eq!(
        decode::<Answer>(&r).unwrap_err(),
        SlackError::RateLimited { retry_after: Some(Duration::from_secs(7)) }
    );
    let r = Raw { status: 429, retry_after: Some("Wed, 21 Oct 2026 07:28:00 GMT".into()), body: Vec::new() };
    assert_eq!(decode::<Answer>(&r).unwrap_err(), SlackError::RateLimited { retry_after: None });
}

#[test]
fn a_retry_after_longer_than_an_hour_is_read_as_an_hour() {
    let hour = Some(Duration::from_secs(3600));
    assert_eq!(retry_after(Some("3600")), hour);
    assert_eq!(retry_after(Some("3601")), hour);
    assert_eq!(retry_after(Some("18446744073709551615")), hour);
    let r = Raw { status: 429, retry_after: Some("99999999".into()), body: Vec::new() };
    assert_eq!(decode::<Answer>(&r).unwrap_err(), SlackError::RateLimited { retry_after: hour });
}

#[test]
fn an_error_code_is_kept_short_and_plain() {
    let long = format!(r#"{{"ok":false,"error":"bad\u001b[31m{}"}}"#, "x".repeat(200));
    let SlackError::Api { code } = decode::<Answer>(&raw(200, &long)).unwrap_err() else { panic!() };
    assert!(code.len() <= 64 && code.chars().all(|c| c.is_ascii_graphic()), "{code}");
    assert_eq!(code, "not_a_slack_code");
    // A server's words or a token where the code goes never reach the log.
    let words = r#"{"ok":false,"error":"see https://x.test/?a=1 lunch at noon"}"#;
    assert_eq!(decode::<Answer>(&raw(200, words)).unwrap_err(), SlackError::Api { code: "not_a_slack_code".into() });
    let token = format!(r#"{{"ok":false,"error":"{}"}}"#, ["xo", "xc-1-2-abc"].concat());
    assert_eq!(decode::<Answer>(&raw(200, &token)).unwrap_err(), SlackError::Api { code: "not_a_slack_code".into() });
    let plain = r#"{"ok":false,"error":"channel_not_found"}"#;
    assert_eq!(decode::<Answer>(&raw(200, plain)).unwrap_err(), SlackError::Api { code: "channel_not_found".into() });
}
