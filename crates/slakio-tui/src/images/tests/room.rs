use super::*;

const MIB: usize = 1024 * 1024;

/// One shown frame drawing `shown`, then what it asked for, each answered and made ready at once
/// as a picture of `png` bytes: how many were asked (of the backend or the disk).
fn frame_sized(images: &mut Images, requests: &mut Requests, shown: &[Source], png: usize) -> usize {
    for source in shown {
        images.look(&ws(), source);
    }
    images.ask(requests, Instant::now());
    let asked = requests.take();
    let looked_up = images.take_lookups().len();
    for r in &asked {
        let job = images.answered(r.id, vec![1]).expect("a job");
        images.decoded(key(&job), Ok(ready(&job, png)));
    }
    asked.len() + looked_up
}

fn parked(images: &Images) -> usize {
    images.entries.values().filter(|e| matches!(e, Entry::Parked { .. })).count()
}

#[test]
fn pictures_larger_than_the_room_holds_are_not_asked_for_past_its_bytes() {
    let (mut images, mut requests) = (on(), Requests::default());
    // Well above BYTES_CAP / ENTRIES_MAX each: the bytes fill the room long before the count.
    let shown: Vec<Source> = (0..30).map(thumb).collect();
    let mut asked = 0;
    for _ in 0..20 {
        asked += frame_sized(&mut images, &mut requests, &shown, MIB);
    }
    assert_eq!(asked, BYTES_CAP / MIB, "asked only what fits");
    assert_eq!(images.held(), (BYTES_CAP / MIB, BYTES_CAP));
    assert_eq!(parked(&images), 0, "none fetched to be thrown away");
    assert!(!images.take_graphics().contains("a=d"), "none shown removed");
    // A scroll that keeps them all on screen and shows one more, and back: nothing asked again.
    let more: Vec<Source> = (0..31).map(thumb).collect();
    for shown in [&more, &more, &shown, &more, &shown, &shown] {
        assert_eq!(frame_sized(&mut images, &mut requests, shown, MIB), 0);
    }
    assert_eq!(images.held(), (BYTES_CAP / MIB, BYTES_CAP));
}

#[test]
fn a_parked_picture_stays_parked_across_scrolls_until_it_would_fit() {
    let (mut images, mut requests) = (on(), Requests::default());
    let held: Vec<Source> = (0..BYTES_CAP / MIB - 1).map(thumb).collect();
    for _ in 0..10 {
        frame_sized(&mut images, &mut requests, &held, MIB);
    }
    assert_eq!(images.held(), (held.len(), BYTES_CAP - MIB));
    images.take_graphics();
    // One more on screen looks as if it fits, and comes twice as large: no room, parked.
    let all: Vec<Source> = held.iter().cloned().chain([thumb(999)]).collect();
    assert_eq!(frame_sized(&mut images, &mut requests, &all, 2 * MIB), 1);
    let big = (thumb(999), Shape::Preview);
    assert_eq!(images.entries.get(&big), Some(&Entry::Parked { bytes: 2 * MIB }));
    assert_eq!(images.take_graphics(), "", "neither sent nor anything removed");
    // A scroll of a line (the first one leaves the screen for a frame) and back: still parked,
    // fetched and decoded no more.
    let scrolled: Vec<Source> = all[1..].to_vec();
    for shown in [&scrolled, &all, &scrolled, &all, &all] {
        assert_eq!(frame_sized(&mut images, &mut requests, shown, 2 * MIB), 0);
        assert_eq!(images.entries.get(&big), Some(&Entry::Parked { bytes: 2 * MIB }));
    }
    // Only it on screen (for two frames, the one before still showed the others): it fits, and
    // is asked for again.
    let alone = [thumb(999)];
    assert_eq!(frame_sized(&mut images, &mut requests, &alone, 2 * MIB), 0);
    assert_eq!(frame_sized(&mut images, &mut requests, &alone, 2 * MIB), 1);
    assert!(matches!(images.look(&ws(), &alone[0]), Look::Ready { .. }));
    assert!(images.held().1 <= BYTES_CAP);
}
