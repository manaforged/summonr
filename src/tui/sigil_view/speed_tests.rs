use std::time::{Duration, Instant};

use crate::tui::sigil_view::speed::Speed;

fn at(base: Instant, millis: u64) -> Instant {
    base + Duration::from_millis(millis)
}

#[test]
fn replayed_output_does_not_measure_read_speed() {
    let base = Instant::now();
    let mut speed = Speed::default();
    speed.delta(400, base);
    assert_eq!(speed.live(base), None);
    speed.finish(100, at(base, 1));
    speed.finish(200, at(base, 2));
    assert_eq!(speed.last(), None);
}

#[test]
fn live_rate_averages_the_recent_window() {
    let base = Instant::now();
    let mut speed = Speed::default();
    speed.start(base);
    assert_eq!(speed.live(at(base, 500)), None);
    speed.delta(40, at(base, 500));
    speed.delta(40, at(base, 1_000));
    speed.delta(40, at(base, 1_500));
    let rate = speed.live(at(base, 1_500)).expect("rate");
    assert!(
        (rate - 30.0).abs() < 0.01,
        "30 tokens over one second: {rate}"
    );
    assert_eq!(
        speed.live(at(base, 6_000)),
        None,
        "a stalled stream shows no live rate"
    );
}

#[test]
fn finish_uses_the_provider_count_and_recalibrates() {
    let base = Instant::now();
    let mut speed = Speed::default();
    speed.start(base);
    speed.delta(200, at(base, 1_000));
    speed.delta(200, at(base, 3_000));
    speed.finish(100, at(base, 3_200));
    let rate = speed.last().expect("round rate");
    assert!(
        (rate - 50.0).abs() < 0.01,
        "100 tokens over two seconds: {rate}"
    );
    speed.delta(80, at(base, 4_000));
    let live = speed.live(at(base, 4_000)).expect("live");
    assert!(
        (live - 80.0).abs() < 0.01,
        "four chars per token over the floor span: {live}"
    );
}

#[test]
fn finish_without_deltas_uses_the_round_span() {
    let base = Instant::now();
    let mut speed = Speed::default();
    speed.start(base);
    speed.finish(30, at(base, 3_000));
    let rate = speed.last().expect("round rate");
    assert!((rate - 10.0).abs() < 0.01, "{rate}");
    speed.finish(0, at(base, 4_000));
    assert!((speed.last().expect("kept") - 10.0).abs() < 0.01);
    assert_eq!(Speed::label(38.4), "38 tok/s");
}
