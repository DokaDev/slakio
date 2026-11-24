use super::super::{Entry, Images, PHOTO, Why};
use slakio_core::images::Source;
use std::time::{Duration, Instant};

fn failed(images: &mut Images, name: &str, again: Option<Instant>) {
    let key = (Source::Avatar(format!("https://example.invalid/{name}.png")), PHOTO);
    images.entries.insert(key, Entry::Failed { why: Why::NotLoaded, again });
}

#[test]
fn the_loop_wakes_once_for_each_failed_picture_s_wait_and_never_for_never() {
    let (mut images, now) = (Images::default(), Instant::now());
    assert_eq!(images.retry_at(), None);
    failed(&mut images, "never", None);
    assert_eq!(images.retry_at(), None, "never asked again: no wake");
    let (soon, later) = (now + Duration::from_secs(2), now + Duration::from_secs(60));
    failed(&mut images, "soon", Some(soon));
    failed(&mut images, "later", Some(later));
    assert_eq!(images.retry_at(), Some(soon));
    assert!(!images.woke(now), "not yet");
    assert!(images.woke(soon), "the wait is over: a frame asks again");
    assert_eq!(images.retry_at(), Some(later), "the next one, not the one passed");
    assert!(!images.woke(soon + Duration::from_secs(1)));
    assert!(images.woke(later));
    assert_eq!(images.retry_at(), None, "no spin on a wait that passed");
}
