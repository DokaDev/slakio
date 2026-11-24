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
    (0..n).map(|i| (graphics::transmit(id, 2, 1, &vec![0; i + 1]), delay)).collect()
}

/// The emoji `count` of, each with `n` frames of `delay`, drawn, fetched and sent.
fn held(count: usize, n: usize, delay: Duration) -> (Images, Instant) {
    let (mut images, mut requests) = (Images { on: true, ..Images::default() }, Requests::default());
    let now = Instant::now();
    for i in 0..count {
        images.look_emoji(&ws(), emoji(i).address());
        images.ask(&mut requests, now);
        for r in requests.take() {
            let job = images.answered(r.id, vec![1]).expect("a job");
            let moving = Sent { frames: frames(job.id, n, delay), seen: n, why: "" };
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
}

#[test]
fn a_picture_removed_from_the_terminal_is_never_sent_a_frame_again() {
    let (mut images, now) = held(1, 3, Duration::from_millis(100));
    draw(&mut images, [0], now);
    images.forget_where(|k| matches!(k.0, Source::Emoji(_)));
    images.take_graphics();
    assert!(run(&mut images, now, SECOND, TICK).is_empty());
}
