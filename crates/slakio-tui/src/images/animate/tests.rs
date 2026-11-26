use super::*;
use crate::app::requests::Requests;
use crate::images::{Look, Prepared, frames};
use slakio_core::images::Source;
use slakio_core::model::WorkspaceId;

fn ws() -> WorkspaceId {
    WorkspaceId::new("T1")
}

fn emoji(n: usize) -> Source {
    Source::Emoji(format!("https://{}/T1/e{n}/x.gif", slakio_core::images::EMOJI_HOSTS[0]))
}

/// Each frame's command says which frame it is (a PNG of `n + 1` bytes), so a test can tell.
fn frames(id: u32, n: usize, delay: Duration) -> Vec<(String, Duration)> {
    sized(id, &vec![delay; n], 0)
}

/// A frame for each of `delays`, frame `i` a PNG of `size + i + 1` bytes.
fn sized(id: u32, delays: &[Duration], size: usize) -> Vec<(String, Duration)> {
    delays.iter().enumerate().map(|(i, d)| (graphics::transmit(id, 2, 1, &vec![0; size + i + 1]), *d)).collect()
}

/// The emoji `count` of, each with `n` frames of `delay`, drawn, fetched and sent.
fn held(count: usize, n: usize, delay: Duration) -> (Images, Instant) {
    held_by(count, &vec![delay; n], 0)
}

/// The emoji `count` of, each with frames of `delays` ([`sized`] by `size`), fetched and sent.
fn held_by(count: usize, delays: &[Duration], size: usize) -> (Images, Instant) {
    let n = delays.len();
    let (mut images, mut requests) = (Images { on: true, ..Images::default() }, Requests::default());
    let now = Instant::now();
    for i in 0..count {
        images.look_emoji(&ws(), emoji(i).address());
        images.ask(&mut requests, now);
        for r in requests.take() {
            let job = images.answered(r.id, vec![1]).expect("a job");
            let moving = Sent { frames: sized(job.id, delays, size), seen: n, why: "" };
            let transmit = graphics::transmit(job.id, 2, 1, &[0]);
            let p = Prepared { id: job.id, cols: 2, rows: 1, bytes: n, transmit, from: (2, 1, "gif"), moving };
            images.decoded((job.source, job.request), Ok(p));
        }
    }
    images.take_graphics();
    (images, now)
}

/// A frame drawn at `now` showing emoji `shown`.
fn draw(images: &mut Images, shown: impl IntoIterator<Item = usize>, now: Instant) {
    for i in shown {
        assert!(matches!(images.look_emoji(&ws(), emoji(i).address()), Look::Ready { .. }), "emoji {i} is held");
    }
    images.ask(&mut Requests::default(), now);
}

/// The loop's ticks every `step` for `span` from `from`, as it wakes: how many frames each
/// picture (by id) was sent.
fn run(images: &mut Images, from: Instant, span: Duration, step: Duration) -> HashMap<u32, usize> {
    let mut sent: HashMap<u32, usize> = HashMap::new();
    let mut t = from;
    while t <= from + span {
        images.advance(t);
        for command in images.take_graphics().split("\x1b\\").filter(|c| c.contains("a=T")) {
            let id = command.split("i=").nth(1).and_then(|r| r.split(',').next()).unwrap().parse().unwrap();
            *sent.entry(id).or_default() += 1;
        }
        t += step;
    }
    sent
}

const SECOND: Duration = Duration::from_secs(1);
const TICK: Duration = Duration::from_millis(10);

/// A frame sent: when, to which picture, which of its frames, how many bytes.
#[derive(Debug)]
struct Sending {
    t: Duration,
    id: u32,
    frame: usize,
    bytes: usize,
}

/// As [`run`], each frame sent as a [`Sending`] (`t` from `start`), frames as [`sized`] made
/// them for `delays` and `size`.
fn log(images: &mut Images, start: Instant, span: Duration, delays: &[Duration], size: usize) -> Vec<Sending> {
    let mut out = Vec::new();
    let mut t = start;
    while t <= start + span {
        images.advance(t);
        // A frame's command may come in pieces: each is matched whole against the frames.
        let mut rest = images.take_graphics();
        while !rest.is_empty() {
            let id = rest.split("i=").nth(1).and_then(|r| r.split(',').next()).unwrap().parse().unwrap();
            let all = sized(id, delays, size);
            let frame = all.iter().position(|f| rest.starts_with(&f.0)).expect("a frame of its own");
            out.push(Sending { t: t - start, id, frame, bytes: all[frame].0.len() });
            rest = rest.split_off(all[frame].0.len());
        }
        t += TICK;
    }
    out
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn a_moving_picture_on_screen_is_sent_its_next_frames_under_its_own_id() {
    let (mut images, now) = held(1, 3, Duration::from_millis(200));
    draw(&mut images, [0], now);
    let at = images.frame_at().expect("a frame due");
    assert!(at >= now + Duration::from_millis(200) && at < now + Duration::from_millis(250), "{at:?}");
    assert!(!images.advance(at - TICK), "not before it is due");
    assert!(images.advance(at));
    let sent = images.take_graphics();
    assert_eq!(sent, frames(1, 3, Duration::ZERO)[1].0, "frame 2, as picture 1, placed again (a=T,U=1)");
    assert!(sent.starts_with("\x1b_Ga=T,U=1,f=100,t=d,i=1,c=2,r=1,q=2"), "{sent:?}");
}

#[test]
fn delay_zero_moves_at_most_ten_frames_a_second() {
    let (mut images, now) = held(1, 4, frames::delay(0, 1));
    draw(&mut images, [0], now);
    let sent = run(&mut images, now, SECOND, TICK);
    let n = sent.get(&1).copied().unwrap_or(0);
    assert!((9..=10).contains(&n), "{n} frames in a second");
}

#[test]
fn only_what_the_screen_shows_moves_and_scrolling_starts_and_stops_it() {
    let (mut images, now) = held(2, 3, Duration::from_millis(100));
    draw(&mut images, [0], now);
    let sent = run(&mut images, now, SECOND, TICK);
    assert!(sent.contains_key(&1) && !sent.contains_key(&2), "the one off screen gets no frame: {sent:?}");
    // Scrolled: the other one in, the first out.
    let later = now + 2 * SECOND;
    draw(&mut images, [1], later);
    let sent = run(&mut images, later, SECOND, TICK);
    assert!(!sent.contains_key(&1) && sent.contains_key(&2), "{sent:?}");
    // Both out: nothing is due, the loop never wakes for them.
    draw(&mut images, [], later + 2 * SECOND);
    assert_eq!(images.frame_at(), None);
    assert!(run(&mut images, later + 2 * SECOND, SECOND, TICK).is_empty());
}

#[test]
fn at_most_eight_move_at_once_the_first_ones_drawn() {
    let (mut images, now) = held(12, 3, Duration::from_millis(100));
    draw(&mut images, 0..12, now);
    let sent = run(&mut images, now, SECOND, TICK);
    let mut ids: Vec<u32> = sent.keys().copied().collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=8).collect::<Vec<u32>>());
}

#[test]
fn nothing_moving_never_wakes_the_loop() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    assert_eq!(images.frame_at(), None, "held, not on screen");
    let (mut still, _) = held(1, 1, Duration::from_millis(100));
    draw(&mut still, [0], now);
    assert_eq!(still.frame_at(), None, "one frame: it does not move");
    draw(&mut images, [0], now);
    assert!(images.frame_at().is_some());
}

#[test]
fn animate_off_puts_each_back_on_its_first_frame_and_nothing_moves() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    draw(&mut images, [0], now);
    run(&mut images, now, Duration::from_millis(150), TICK);
    images.set_animate(false);
    assert_eq!(images.take_graphics(), frames(1, 3, Duration::ZERO)[0].0, "the first frame again");
    draw(&mut images, [0], now + SECOND);
    assert_eq!(images.frame_at(), None);
    assert!(run(&mut images, now + SECOND, SECOND, TICK).is_empty());
    images.set_animate(true);
    draw(&mut images, [0], now + 3 * SECOND);
    assert!(!run(&mut images, now + 3 * SECOND, SECOND, TICK).is_empty(), "on again, it moves");
}

#[test]
fn animate_off_from_the_start_sends_only_the_first_frame() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    images.set_animate(false);
    draw(&mut images, [0], now);
    assert!(run(&mut images, now, SECOND, TICK).is_empty());
    assert_eq!(images.take_graphics(), "");
}

#[test]
fn pictures_off_stops_every_frame() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    draw(&mut images, [0], now);
    images.tty = true;
    images.setting = "off".to_string();
    images.resolve();
    assert!(images.take_graphics().contains("a=d,d=A"), "every picture leaves the terminal");
    assert_eq!(images.frame_at(), None);
    assert!(run(&mut images, now, SECOND, TICK).is_empty(), "no frame after");
    assert!(images.anims.list.is_empty() && images.anims.moving.is_empty(), "no frame is kept either");
}

#[test]
fn a_picture_removed_from_the_terminal_is_never_sent_a_frame_again() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    draw(&mut images, [0], now);
    images.forget_where(|k| matches!(k.0, Source::Emoji(_)));
    images.take_graphics();
    assert!(run(&mut images, now, SECOND, TICK).is_empty());
}

#[test]
fn a_late_wake_does_not_push_the_next_frame_back() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    draw(&mut images, [0], now);
    let at = images.frame_at().expect("a frame due");
    assert!(images.advance(at + Duration::from_millis(1)));
    assert_eq!(images.frame_at(), Some(at + Duration::from_millis(100)), "from when it was due, not from the wake");
}

#[test]
fn a_coarse_tick_from_an_offset_start_still_shows_ten_frames_a_second() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    draw(&mut images, [0], now);
    let sent = run(&mut images, now + Duration::from_millis(7), 10 * SECOND, Duration::from_millis(13));
    let n = sent.get(&1).copied().unwrap_or(0);
    assert!((98..=101).contains(&n), "{n} frames in ten seconds");
}

#[test]
fn a_picture_under_a_covering_popup_stops() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    draw(&mut images, [0], now);
    assert!(images.frame_at().is_some());
    images.cover(true);
    draw(&mut images, [0], now);
    assert_eq!(images.frame_at(), None, "drawn under the popup, not seen");
    assert!(run(&mut images, now, SECOND, TICK).is_empty());
    images.cover(false);
    draw(&mut images, [0], now + SECOND);
    assert!(images.frame_at().is_some(), "uncovered, it moves again");
}

#[test]
fn a_forty_ms_gif_advances_every_forty_ms() {
    let delays = [ms(40); 10];
    let (mut images, now) = held_by(1, &delays, 0);
    draw(&mut images, [0], now);
    let sent = log(&mut images, now, SECOND, &delays, 0);
    assert!((24..=26).contains(&sent.len()), "{} frames in a second", sent.len());
    for (i, s) in sent.iter().enumerate() {
        assert_eq!(s.t, ms(40) * (i as u32 + 1), "frame {i} on time");
        assert_eq!(s.frame, (i + 1) % 10);
    }
}

#[test]
fn a_zero_ms_gif_plays_at_a_hundred_ms_and_a_twenty_ms_one_at_twenty() {
    let delays = [frames::delay(0, 1), frames::delay(20, 1), frames::delay(0, 1), frames::delay(20, 1)];
    let (mut images, now) = held_by(1, &delays, 0);
    draw(&mut images, [0], now);
    let sent = log(&mut images, now, ms(2400), &delays, 0);
    // Ten cycles of 240 ms, four frames each (at ten frames a second it would be 24).
    assert_eq!(sent.len(), 40, "frames in 2.4 s");
    // Frame 0 shows 100 ms, frame 1 20 ms, and so on.
    let times: Vec<u64> = sent.iter().take(4).map(|s| s.t.as_millis() as u64).collect();
    assert_eq!(times, [100, 120, 220, 240]);
}

#[test]
fn sixty_ms_frames_never_drift_over_ten_seconds() {
    let delays = [ms(60); 3];
    let (mut images, now) = held_by(1, &delays, 0);
    draw(&mut images, [0], now);
    let sent = log(&mut images, now, 10 * SECOND, &delays, 0);
    assert!((166..=167).contains(&sent.len()), "{} frames in ten seconds", sent.len());
    assert_eq!(sent.last().map(|s| s.t), Some(ms(60 * 166)), "the last one on time");
}

#[test]
fn a_late_wake_skips_to_the_frame_due_now() {
    let delays = [ms(40); 10];
    let (mut images, now) = held_by(1, &delays, 0);
    draw(&mut images, [0], now);
    assert!(images.advance(now + ms(330)));
    assert_eq!(images.take_graphics(), sized(1, &delays, 0)[8].0, "only the frame due at 330 ms");
    assert_eq!(images.frame_at(), Some(now + ms(360)));
}

#[test]
fn a_wake_many_cycles_late_costs_one_frame_and_no_loop() {
    let delays = [ms(40); 10];
    let (mut images, now) = held_by(1, &delays, 0);
    draw(&mut images, [0], now);
    let late = now + 3600 * SECOND + ms(130);
    assert!(images.advance(late));
    assert_eq!(images.take_graphics(), sized(1, &delays, 0)[3].0, "one frame, the one due");
    assert_eq!(images.frame_at(), Some(late + ms(30)));
}

#[test]
fn over_the_byte_budget_frames_are_skipped_and_animation_time_is_kept() {
    let (delays, size) = ([ms(20); 10], 15_000);
    let (mut images, now) = held_by(8, &delays, size);
    draw(&mut images, 0..8, now);
    let sent = log(&mut images, now, 2 * SECOND, &delays, size);
    let frame = sent.iter().map(|s| s.bytes).max().unwrap();
    assert!((19_000..=21_000).contains(&frame), "{frame} bytes a frame");
    let budget = super::budget::ANIM_BYTES_PER_SECOND;
    let total: usize = sent.iter().map(|s| s.bytes).sum();
    assert!(total <= 2 * budget + frame, "{total} bytes in two seconds");
    for s in &sent {
        let window: usize = sent.iter().filter(|o| o.t <= s.t && o.t + SECOND > s.t).map(|o| o.bytes).sum();
        assert!(window <= budget, "{window} bytes in the second up to {:?}", s.t);
        assert_eq!(s.frame, (s.t.as_millis() as usize / 20) % 10, "the frame due when sent: {s:?}");
    }
    assert!(sent.len() < 8 * 100, "frames were skipped");
    for id in 1..=8 {
        assert!(sent.iter().any(|s| s.id == id), "picture {id} moved");
    }
    // Never slowed: each picture is on the frame due at 2 s, and wakes for the next on time.
    assert!(images.anims.list.values().all(|a| a.at == (2000 / 20) % 10), "on frame 0 again");
    assert_eq!(images.frame_at(), Some(now + ms(2020)));
}

#[test]
fn under_the_budget_every_frame_is_sent() {
    let (delays, size) = ([ms(40); 10], 2_200);
    let (mut images, now) = held_by(1, &delays, size);
    draw(&mut images, [0], now);
    let sent = log(&mut images, now, SECOND, &delays, size);
    assert!(sent.iter().all(|s| (2_900..=3_100).contains(&s.bytes)), "about 3 KB a frame");
    assert_eq!(sent.len(), 25);
}
