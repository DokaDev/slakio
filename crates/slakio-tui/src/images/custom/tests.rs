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
