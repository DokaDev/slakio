use super::*;
use slakio_core::images::{AVATAR_HOSTS, EMOJI_HOSTS};

fn photo() -> Source {
    Source::Avatar(format!("https://{}/2026-01-05/T1-U1_72.png", AVATAR_HOSTS[0]))
}

fn emoji() -> Source {
    Source::Emoji(format!("https://{}/T1/e/0.png", EMOJI_HOSTS[0]))
}

fn preview() -> Source {
    Source::Thumb(format!("https://{}/files-tmb/T1-F1/x.png", slakio_core::model::FILE_HOST))
}

const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn a_photo_or_emoji_that_failed_for_a_while_waits_two_five_then_fifteen_seconds() {
    for source in [photo(), emoji()] {
        for kind in [FailKind::CapReached, FailKind::Busy, FailKind::Offline, FailKind::Network, FailKind::Stalled] {
            let waits: Vec<_> = (1..=5).map(|t| wait(&source, &kind, t)).collect();
            assert_eq!(
                waits,
                [Some(secs(2)), Some(secs(5)), Some(secs(15)), Some(secs(15)), Some(secs(15))],
                "{kind:?}"
            );
        }
        assert_eq!(wait(&source, &FailKind::CapReached, u8::MAX), Some(secs(15)), "never past the last");
        assert_eq!(wait(&source, &FailKind::CapReached, 0), Some(secs(2)), "no try counted: the first");
    }
}

#[test]
fn a_host_s_own_wait_is_kept_when_longer_and_at_most_an_hour() {
    let said = |s: u64| FailKind::RateLimited { retry_after: Some(secs(s)) };
    assert_eq!(wait(&photo(), &said(40), 1), Some(secs(40)));
    assert_eq!(wait(&photo(), &said(1), 3), Some(secs(15)), "never shorter than its turn");
    assert_eq!(wait(&emoji(), &said(86_400), 1), Some(secs(3600)));
    assert_eq!(wait(&emoji(), &FailKind::RateLimited { retry_after: None }, 2), Some(secs(5)));
}

#[test]
fn what_cannot_be_shown_is_never_asked_again() {
    for kind in [FailKind::TooLarge, FailKind::Api { code: "file_not_available".into() }, FailKind::Auth] {
        for source in [photo(), emoji(), preview()] {
            assert_eq!(wait(&source, &kind, 1), None, "{kind:?}");
        }
    }
}

#[test]
fn a_preview_waits_as_it_always_has() {
    assert_eq!(wait(&preview(), &FailKind::Busy, 3), Some(RETRY_SOON));
    assert_eq!(wait(&preview(), &FailKind::Offline, 1), Some(RETRY_SOON));
    assert_eq!(wait(&preview(), &FailKind::CapReached, 1), Some(RETRY_AFTER));
    assert_eq!(wait(&preview(), &FailKind::Network, 2), Some(RETRY_AFTER));
}
