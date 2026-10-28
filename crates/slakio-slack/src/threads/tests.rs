use super::*;
use slakio_core::sanitize::Remote;

fn message(ts: u64, text: &str) -> Message {
    Message {
        ts: Ts(ts),
        user: UserId::new("U1"),
        text: Remote::new(text),
        thread: None,
        reactions: Vec::new(),
        edited: false,
    }
}

fn walked(n: u64, text: &str) -> Walked {
    Walked { messages: (0..n).map(|i| message(i, text)).collect(), bots: Bots::new(), cut: false }
}

fn key(n: u64) -> Key {
    ("C1".to_string(), Ts(n))
}

#[test]
fn the_threads_used_longest_ago_are_dropped_past_the_count() {
    let threads = Threads::with_limits(2, usize::MAX);
    threads.keep(key(1), walked(1, "a"));
    threads.keep(key(2), walked(1, "a"));
    // Using thread 1 (an older page of it) makes 2 the one used longest ago.
    threads.slice(&key(1), Some(Ts(9)), 28);
    threads.keep(key(3), walked(1, "a"));
    assert_eq!(threads.kept().0, [key(1), key(3)]);
}

#[test]
fn the_threads_kept_hold_at_most_their_bytes() {
    let one = walked(10, &"x".repeat(1000)).bytes();
    let threads = Threads::with_limits(64, one * 2);
    for n in 1..=3 {
        threads.keep(key(n), walked(10, &"x".repeat(1000)));
    }
    let (order, bytes) = threads.kept();
    assert_eq!(order, [key(2), key(3)]);
    assert_eq!(bytes, one * 2);
    // One thread bigger than all the room is kept all the same (it is the one on screen), alone.
    threads.keep(key(4), walked(30, &"x".repeat(1000)));
    let (order, bytes) = threads.kept();
    assert_eq!(order, [key(4)]);
    assert_eq!(bytes, walked(30, &"x".repeat(1000)).bytes());
    // Kept again: counted once.
    threads.keep(key(4), walked(1, "y"));
    assert_eq!(threads.kept().1, walked(1, "y").bytes());
}

#[test]
fn what_is_read_from_the_last_reply_on_goes_onto_what_was_kept() {
    let old = Walked { messages: vec![message(1, "parent"), message(2, "a"), message(3, "b")], ..Walked::default() };
    let new = Walked {
        messages: vec![message(1, "parent"), message(3, "b, edited"), message(4, "c")],
        cut: true,
        ..Walked::default()
    };
    let all = merged(old, new, Ts(3));
    let texts: Vec<&str> = all.messages.iter().map(|m| m.text.unsanitized()).collect();
    assert_eq!(texts, ["parent", "a", "b, edited", "c"]);
    assert!(all.cut);
}
