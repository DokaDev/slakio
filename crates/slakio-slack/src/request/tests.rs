//! Golden tests: the exact requests, against synthetic credentials and fixed ids and time.

use super::*;

/// A synthetic token (spelled in two parts so no file holds a token-shaped string).
const TOKEN: &str = concat!("xo", "xc-TEST");

struct FixedIds;

impl Ids for FixedIds {
    fn hex(&mut self, bytes: usize) -> String {
        "ab".repeat(bytes)
    }
}

fn setup(team: Option<&str>) -> (Credentials, Session, Endpoints) {
    let mut session = Session::new(&mut FixedIds);
    if let Some(t) = team {
        session.set_team(t);
    }
    let endpoints = Endpoints::local("127.0.0.1:9".parse().unwrap()).unwrap();
    (Credentials::new(TOKEN, "cookie-TEST%2F1"), session, endpoints)
}

const MAC_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36";

fn header_names(p: &Prepared) -> Vec<&'static str> {
    p.headers.iter().map(|(n, _)| *n).collect()
}

#[test]
fn a_boot_request_before_sign_in_matches_the_web_client() {
    let (creds, session, endpoints) = setup(None);
    let call = Call::form(
        Method::ClientUserBoot,
        &[("version_all_channels", "false"), ("return_all_relevant_mpdms", "true"), ("omit_extras", "a,b")],
    );
    let p = prepare(&call, &creds, &session, &endpoints, 1_767_603_600_042, &mut FixedIds, Os::Mac).unwrap();
    assert_eq!(
        p.url,
        "http://127.0.0.1:9/api/client.userBoot?_x_id=noversion-1767603600.042&_x_version_ts=1785403654\
         &_x_foreground=true&_x_frontend_build_type=current&_x_desktop_ia=4&_x_gantry=true&fp=6e&_x_num_retries=0"
    );
    assert_eq!(
        p.body,
        format!(
            "token={TOKEN}&version_all_channels=false&return_all_relevant_mpdms=true&omit_extras=a%2Cb\
             &_x_reason=initial-data&_x_sonic=true&_x_app_name=client"
        )
    );
    assert_eq!(
        p.headers,
        vec![
            ("User-Agent", MAC_UA.to_string()),
            ("Accept", "*/*".to_string()),
            ("Accept-Language", "en-US,en;q=0.9".to_string()),
            ("Origin", "https://app.slack.com".to_string()),
            ("Sec-Fetch-Site", "same-site".to_string()),
            ("Sec-Fetch-Mode", "cors".to_string()),
            ("Sec-Fetch-Dest", "empty".to_string()),
            ("Sec-Ch-Ua", "\"Not;A=Brand\";v=\"8\", \"Chromium\";v=\"150\", \"Google Chrome\";v=\"150\"".to_string()),
            ("Sec-Ch-Ua-Mobile", "?0".to_string()),
            ("Sec-Ch-Ua-Platform", "\"macOS\"".to_string()),
            ("Cache-Control", "no-cache".to_string()),
            ("Pragma", "no-cache".to_string()),
            ("Priority", "u=1, i".to_string()),
            ("Content-Type", "application/x-www-form-urlencoded".to_string()),
            ("Cookie", "d=cookie-TEST%2F1".to_string()),
        ]
    );
}

#[test]
fn after_sign_in_the_query_carries_the_session_route_and_trace() {
    let (creds, session, endpoints) = setup(Some("T0TEST"));
    let call = Call::form(Method::ClientCounts, &[]);
    let p = prepare(&call, &creds, &session, &endpoints, 1_767_603_600_005, &mut FixedIds, Os::Linux).unwrap();
    let trace = "ab".repeat(16);
    let span = "ab".repeat(8);
    assert_eq!(
        p.url,
        format!(
            "http://127.0.0.1:9/api/client.counts?_x_id=abababab-1767603600.005&_x_csid=abababababa\
             &slack_route=T0TEST&_x_version_ts=1785403654&_x_foreground=true&_x_frontend_build_type=current\
             &_x_desktop_ia=4&_x_gantry=true&_x_b3_traceid={trace}&_x_b3_spanid={span}&_x_b3_sampled=1\
             &fp=6e&_x_num_retries=0"
        )
    );
    assert_eq!(
        p.body,
        format!("token={TOKEN}&_x_reason=fetchClientCountsOnConnect&_x_mode=online&_x_sonic=true&_x_app_name=client")
    );
    assert!(p.headers.iter().any(|h| *h == ("Sec-Ch-Ua-Platform", "\"Linux\"".to_string())));
}

#[test]
fn a_view_sends_no_reason_and_no_mode() {
    let (creds, session, endpoints) = setup(Some("T0TEST"));
    let call = Call::form(Method::ConversationsView, &[("channel", "C0TEST"), ("count", "28")]);
    let p = prepare(&call, &creds, &session, &endpoints, 0, &mut FixedIds, Os::Mac).unwrap();
    assert_eq!(p.body, format!("token={TOKEN}&channel=C0TEST&count=28&_x_sonic=true&_x_app_name=client"));
}

#[test]
fn the_token_is_never_a_header_and_no_referer_or_encoding_is_sent() {
    let (creds, session, endpoints) = setup(Some("T0TEST"));
    for m in Method::ALL {
        let call = if m.edge() { Call::json(*m, serde_json::Map::new()) } else { Call::form(*m, &[]) };
        let p = prepare(&call, &creds, &session, &endpoints, 0, &mut FixedIds, Os::Mac).unwrap();
        let names = header_names(&p);
        for absent in ["Authorization", "Referer", "Accept-Encoding"] {
            assert!(!names.contains(&absent), "{m:?} sends {absent}");
        }
        assert!(p.headers.iter().all(|(_, v)| !v.contains(TOKEN)), "{m:?}");
        assert!(!p.url.contains(TOKEN), "{m:?}");
        assert!(p.body.contains(TOKEN), "{m:?}");
    }
}

#[test]
fn an_edge_lookup_has_the_small_query_and_a_sorted_json_body() {
    let (creds, session, endpoints) = setup(Some("T0TEST"));
    let mut body = serde_json::Map::new();
    body.insert("updated_ids".into(), serde_json::json!({"C0TEST": 1}));
    body.insert("check_interaction".into(), serde_json::json!(true));
    let p =
        prepare(&Call::json(Method::EdgeChannelsInfo, body), &creds, &session, &endpoints, 0, &mut FixedIds, Os::Mac)
            .unwrap();
    let (trace, span) = ("ab".repeat(16), "ab".repeat(8));
    assert_eq!(
        p.url,
        format!(
            "http://127.0.0.1:9/cache/T0TEST/channels/info?_x_app_name=client&_x_b3_traceid={trace}\
             &_x_b3_spanid={span}&_x_b3_sampled=1&fp=6e&_x_num_retries=0"
        )
    );
    assert_eq!(
        p.body,
        format!("{{\"check_interaction\":true,\"token\":\"{TOKEN}\",\"updated_ids\":{{\"C0TEST\":1}}}}")
    );
    assert!(p.headers.contains(&("Content-Type", "text/plain;charset=UTF-8".to_string())));
}

#[test]
fn an_edge_lookup_before_sign_in_is_refused() {
    let (creds, session, endpoints) = setup(None);
    let call = Call::json(Method::EdgeUsersInfo, serde_json::Map::new());
    assert!(prepare(&call, &creds, &session, &endpoints, 0, &mut FixedIds, Os::Mac).is_err());
}

#[test]
fn only_a_loopback_address_is_an_endpoint() {
    assert!(Endpoints::local("127.0.0.1:1".parse().unwrap()).is_some());
    assert!(Endpoints::local("[::1]:1".parse().unwrap()).is_some());
    assert!(Endpoints::local("192.0.2.1:443".parse().unwrap()).is_none());
}

#[test]
fn form_encoding_is_the_browsers() {
    assert_eq!(encode("a b,c/d~e_f.g-h"), "a+b%2Cc%2Fd~e_f.g-h");
    assert_eq!(encode("\u{e9}"), "%C3%A9");
}

#[test]
fn random_ids_have_the_asked_length() {
    let mut ids = RandomIds::default();
    let (a, b) = (ids.hex(16), ids.hex(16));
    assert_eq!(a.len(), 32);
    assert_ne!(a, b);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
}
