use super::*;

fn ws(id: &str) -> WorkspaceId {
    WorkspaceId::new(id)
}

fn asked(r: &mut Requests) -> Vec<(Generation, Option<WorkspaceId>)> {
    r.take().into_iter().filter(|q| q.command == Command::Emoji).map(|q| (q.id, q.workspace)).collect()
}

#[test]
fn each_workspace_s_list_is_asked_once_and_only_for_a_name_with_a_letter() {
    let (mut c, mut r, now) = (Customs::default(), Requests::default(), Instant::now());
    assert_eq!(c.get(&ws("T1"), "30"), None);
    assert!(!c.ask(&mut r, now), "digits only: no emoji's");
    for _ in 0..3 {
        c.get(&ws("T1"), "party");
    }
    c.get(&ws("T2"), "logo");
    assert!(c.ask(&mut r, now));
    let first = asked(&mut r);
    assert_eq!(first.iter().map(|(_, w)| w.clone()).collect::<Vec<_>>(), [Some(ws("T1")), Some(ws("T2"))]);
    c.get(&ws("T1"), "party");
    assert!(!c.ask(&mut r, now), "on its way: not again");
    let blob = Custom::Picture("x".into());
    assert!(c.answered(first[0].0, vec![("party".into(), blob.clone())]));
    assert!(!c.answered(first[0].0, Vec::new()), "answered once");
    assert_eq!(c.get(&ws("T1"), "party"), Some(&blob));
    assert_eq!(c.get(&ws("T1"), "other"), None);
    assert!(!c.ask(&mut r, now), "the list is here: a name it lacks asks nothing");
}

#[test]
fn signed_in_again_the_list_is_read_again_when_next_wanted() {
    let (mut c, mut r, now) = (Customs::default(), Requests::default(), Instant::now());
    c.get(&ws("T1"), "party");
    c.ask(&mut r, now);
    let id = asked(&mut r)[0].0;
    c.answered(id, vec![("party".into(), Custom::Standard("tada".into()))]);
    c.forget(&ws("T1"));
    assert_eq!(c.get(&ws("T1"), "party"), None);
    assert!(c.ask(&mut r, now));
    assert_eq!(asked(&mut r).len(), 1);
}

#[test]
fn a_failed_list_waits_by_its_kind() {
    let (mut c, mut r, now) = (Customs::default(), Requests::default(), Instant::now());
    for (kind, wait) in [
        (FailKind::Busy, Some(super::super::RETRY_SOON)),
        (FailKind::Network, Some(super::super::RETRY_AFTER)),
        (FailKind::CapReached, Some(super::super::RETRY_AFTER)),
        (FailKind::Api { code: "missing_scope".into() }, None),
    ] {
        c.forget(&ws("T1"));
        c.get(&ws("T1"), "party");
        c.ask(&mut r, now);
        let id = asked(&mut r)[0].0;
        assert!(c.failed(id, &kind, now));
        assert!(!c.failed(id, &kind, now), "once");
        c.get(&ws("T1"), "party");
        assert!(!c.ask(&mut r, now), "{kind:?}: not at once");
        c.get(&ws("T1"), "party");
        let later = now + wait.unwrap_or(super::super::RETRY_AFTER * 100) + std::time::Duration::from_millis(1);
        assert_eq!(c.ask(&mut r, later), wait.is_some(), "{kind:?}");
        if let Some((id, _)) = asked(&mut r).first() {
            c.answered(*id, Vec::new());
        }
    }
}

#[test]
fn a_shortcode_only_laid_out_asks_for_no_list() {
    let (mut c, mut r, now) = (Customs::default(), Requests::default(), Instant::now());
    let from = c.mark();
    c.get(&ws("T1"), "party");
    c.not_shown(from, c.mark());
    assert!(!c.ask(&mut r, now), "measured off screen: nothing asked");
    let from = c.mark();
    c.get(&ws("T1"), "party");
    c.not_shown(from, c.mark());
    c.get(&ws("T1"), "party");
    assert!(c.ask(&mut r, now), "shown once: asked");
    assert_eq!(asked(&mut r).len(), 1);
}

#[test]
fn a_name_of_korean_letters_only_asks_for_the_list() {
    let (mut c, mut r, now) = (Customs::default(), Requests::default(), Instant::now());
    assert_eq!(c.get(&ws("T1"), "\u{CD95}\u{D558}\u{CD95}\u{D558}"), None);
    assert!(c.ask(&mut r, now), "a Hangul letter is a letter");
    assert_eq!(asked(&mut r).len(), 1);
    c.get(&ws("T2"), "\u{0663}\u{0660}");
    assert!(!c.ask(&mut r, now), "digits of another script only: still no emoji's");
}

#[test]
fn with_the_diagnostics_on_the_list_says_wanted_asked_applied_and_failed_by_a_hash() {
    let dir = std::env::temp_dir().join(format!("slakio-tui-diag-custom-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("pictures.log");
    let diag = Diag::to(path.clone());
    let (mut c, mut r, now) =
        (Customs { diag: diag.clone(), ..Customs::default() }, Requests::default(), Instant::now());
    c.get(&ws("TSECRET1"), "secret_party");
    c.get(&ws("TSECRET2"), "secret_party");
    assert!(c.ask(&mut r, now));
    let ids = asked(&mut r);
    let blob = Custom::Picture("https://example.invalid/secret.png".into());
    assert!(c.answered(ids[0].0, vec![("secret_party".into(), blob), ("yes".into(), Custom::Standard("+1".into()))]));
    assert!(c.failed(ids[1].0, &FailKind::Api { code: "internal_error".into() }, now));
    c.get(&ws("TSECRET2"), "secret_party");
    c.ask(&mut r, now);
    diag.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    let (one, two) = (diag.id("TSECRET1"), diag.id("TSECRET2"));
    for line in [
        format!("emoji-list wanted ui ws={one}"),
        format!("emoji-list asked ui ws={one}"),
        format!("emoji-list applied ok ws={one} names=2 pictures=1"),
        format!("emoji-list failed api-internal_error ws={two} retry=60"),
        format!("emoji-list skipped barred ws={two} retry=60"),
    ] {
        assert!(text.contains(&line), "no `{line}` in\n{text}");
    }
    assert!(!text.contains("SECRET") && !text.contains("secret") && !text.contains("example"), "{text}");
}

/// Ask workspace `T1`'s list at `at` and fail it with `kind`; when it may be asked again.
fn fail_at(c: &mut Customs, r: &mut Requests, at: Instant, kind: &FailKind) -> Option<Instant> {
    c.get(&ws("T1"), "party");
    assert!(c.ask(r, at), "asked at {at:?}");
    let id = asked(r)[0].0;
    assert!(c.failed(id, kind, at));
    c.failed.get(&ws("T1")).copied().flatten()
}

#[test]
fn slack_s_passing_errors_are_asked_again_a_few_times_and_a_refusal_never() {
    use super::super::RETRY_AFTER;
    let (mut c, mut r, now) = (Customs::default(), Requests::default(), Instant::now());
    let passing = FailKind::Api { code: "internal_error".into() };
    let first = fail_at(&mut c, &mut r, now, &passing).expect("asked again");
    assert_eq!(first - now, RETRY_AFTER);
    let second = fail_at(&mut c, &mut r, first, &passing).expect("asked again");
    assert_eq!(second - first, RETRY_AFTER * 2, "twice as long");
    assert_eq!(fail_at(&mut c, &mut r, second, &passing), None, "the third in a row is the last");
    c.get(&ws("T1"), "party");
    assert!(!c.ask(&mut r, second + BACKOFF_MAX * 10));
    for code in ["not_authed", "invalid_auth", "missing_scope", "not_allowed_token_type", "account_inactive"] {
        c.forget(&ws("T1"));
        let kind = FailKind::Api { code: code.into() };
        assert_eq!(fail_at(&mut c, &mut r, now, &kind), None, "{code}: never");
    }
    c.forget(&ws("T1"));
    assert_eq!(fail_at(&mut c, &mut r, now, &FailKind::Auth), None);
    // A success ends the run of failures.
    c.forget(&ws("T1"));
    let at = fail_at(&mut c, &mut r, now, &passing).unwrap();
    c.get(&ws("T1"), "party");
    assert!(c.ask(&mut r, at));
    let id = asked(&mut r)[0].0;
    assert!(c.answered(id, Vec::new()));
    assert_eq!(c.tries.get(&ws("T1")), None);
}

#[test]
fn a_list_that_keeps_failing_waits_longer_each_time_up_to_a_cap_and_heeds_slack_s_pause() {
    use super::super::RETRY_AFTER;
    use std::time::Duration;
    let (mut c, mut r, mut at) = (Customs::default(), Requests::default(), Instant::now());
    let mut waits = Vec::new();
    for _ in 0..8 {
        let again = fail_at(&mut c, &mut r, at, &FailKind::Network).expect("the network is asked again");
        waits.push((again - at).as_secs());
        at = again;
    }
    assert_eq!(waits, [60, 120, 240, 480, 900, 900, 900, 900], "doubling, at most {BACKOFF_MAX:?}");
    c.forget(&ws("T1"));
    let slow = FailKind::RateLimited { retry_after: Some(Duration::from_secs(600)) };
    let now = Instant::now();
    assert_eq!(fail_at(&mut c, &mut r, now, &slow).unwrap() - now, Duration::from_secs(600), "Slack's pause");
    c.forget(&ws("T1"));
    let quick = FailKind::RateLimited { retry_after: Some(Duration::from_secs(5)) };
    assert_eq!(fail_at(&mut c, &mut r, now, &quick).unwrap() - now, RETRY_AFTER, "never sooner than the wait");
    c.forget(&ws("T1"));
    let long = FailKind::RateLimited { retry_after: Some(Duration::from_secs(7200)) };
    assert_eq!(fail_at(&mut c, &mut r, now, &long).unwrap() - now, BACKOFF_MAX);
}

#[test]
fn a_name_the_list_has_not_is_told_once_a_workspace() {
    let dir = std::env::temp_dir().join(format!("slakio-tui-diag-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("pictures.log");
    let diag = Diag::to(path.clone());
    let (mut c, mut r, now) =
        (Customs { diag: diag.clone(), ..Customs::default() }, Requests::default(), Instant::now());
    c.get(&ws("T1"), "party");
    c.ask(&mut r, now);
    let id = asked(&mut r)[0].0;
    c.answered(id, vec![("party".into(), Custom::Standard("tada".into()))]);
    for name in ["gone", "other", "30"] {
        assert_eq!(c.get(&ws("T1"), name), None);
    }
    diag.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    let line = format!("emoji-list missing ui ws={}", diag.id("T1"));
    assert_eq!(text.matches(&line).count(), 1, "once: {text}");
    let _ = std::fs::remove_dir_all(&dir);
}
