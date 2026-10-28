use super::*;
use slakio_core::i18n::Label;

fn shown(s: &Status, now: Instant) -> Option<Msg> {
    s.notice(now).map(|n| n.msg.clone())
}

/// Every notice the slot shows from `now` on, in order, as time passes.
fn seen(mut s: Status, mut now: Instant) -> Vec<Msg> {
    let mut out: Vec<Msg> = shown(&s, now).into_iter().collect();
    while let Some(at) = s.deadline() {
        now = at;
        if !s.expire(now) {
            break;
        }
        out.extend(shown(&s, now));
    }
    out
}

fn label(l: Label) -> Msg {
    Msg::Label(l)
}

#[test]
fn a_queued_notice_taken_from_the_slot_by_a_failure_is_shown_after_it() {
    // An offline start with a damaged cache: the cache's notice, then the boot fails at once.
    let now = Instant::now();
    let mut s = Status::default();
    s.queue(label(Label::CacheRebuilt), Level::Warning, now);
    s.show(label(Label::FailOffline), Level::Warning, now);
    assert_eq!(shown(&s, now), Some(label(Label::FailOffline)), "why it is not refreshed, first");
    assert_eq!(seen(s, now), [label(Label::FailOffline), label(Label::CacheRebuilt)]);
}

#[test]
fn two_notices_at_once_are_each_shown() {
    let now = Instant::now();
    let mut s = Status::default();
    s.queue(label(Label::CacheRebuilt), Level::Warning, now);
    s.queue(label(Label::CacheBackupFailed), Level::Warning, now);
    assert_eq!(seen(s, now), [label(Label::CacheRebuilt), label(Label::CacheBackupFailed)]);
}

#[test]
fn a_notice_queued_after_a_sign_in_leaves_the_sign_in_seen() {
    let now = Instant::now();
    let mut s = Status::default();
    let signed_in = Msg::SigninSignedIn { workspace: "Fake".into() };
    s.show(signed_in.clone(), Level::Info, now);
    s.queue(label(Label::CacheReadOnly), Level::Warning, now);
    assert_eq!(shown(&s, now), Some(signed_in.clone()));
    assert_eq!(seen(s, now), [signed_in, label(Label::CacheReadOnly)]);
}

#[test]
fn a_notice_shown_is_replaced_as_before_and_a_weaker_one_does_not_cover_a_warning() {
    let now = Instant::now();
    let mut s = Status::default();
    s.show(label(Label::FailOffline), Level::Warning, now);
    s.show(label(Label::StatusNothingToRetry), Level::Info, now);
    assert_eq!(shown(&s, now), Some(label(Label::FailOffline)));
    s.show(label(Label::FailNetwork), Level::Warning, now);
    assert_eq!(seen(s, now), [label(Label::FailNetwork)], "a notice not queued is not shown again");
}
