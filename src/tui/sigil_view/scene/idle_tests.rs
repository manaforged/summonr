use sigil::profile::{self, BackendStatus, Counter};
use sigil::reactive::Runtime;

const SIZE: (u16, u16) = (223, 67);
const BEAT: std::time::Duration = std::time::Duration::from_millis(22);

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(u64::MAX)
}

#[test]
fn an_idle_splash_frame_repaints_only_what_twinkles() {
    Runtime::scope("splash-idle", || {
        let (_app, mut host, mut screen) = super::tests::splash_screen(SIZE.0, SIZE.1);
        let mut router = sigil::events::EventRouter::new();
        let mut written = Vec::new();
        for _ in 0..5 {
            screen.frame(&mut host, &mut router, [], SIZE).unwrap();
            screen.flush(&mut std::io::sink()).unwrap();
        }
        let (mut nodes, mut cells, mut bytes) = (Vec::new(), Vec::new(), Vec::new());
        profile::enable();
        for _ in 0..40 {
            screen.frame(&mut host, &mut router, [], SIZE).unwrap();
            screen.flush(&mut std::io::sink()).unwrap();
            std::thread::park_timeout(BEAT);
            profile::reset();
            profile::begin_frame(BackendStatus::TERMINAL);
            screen.frame(&mut host, &mut router, [], SIZE).unwrap();
            written.clear();
            screen.flush(&mut written).unwrap();
            profile::tick_frame();
            let Some(frame) = profile::frames().pop() else {
                continue;
            };
            if frame.counter(Counter::TuiDirty) > 0 {
                nodes.push(frame.counter(Counter::EmitNodes));
                cells.push(frame.counter(Counter::TuiCellsRastered));
                bytes.push(written.len() as u64);
            }
        }
        profile::disable();
        assert!(
            nodes.len() >= 5,
            "the splash changed on {} of 40 frames",
            nodes.len()
        );
        let most = nodes.iter().copied().max().unwrap_or(u64::MAX);
        assert!(
            most <= 480,
            "a twinkle after an idle frame re-emitted {most} nodes of about 960"
        );
        let (nodes, cells, bytes) = (median(nodes), median(cells), median(bytes));
        assert!(
            nodes <= 300,
            "a twinkle re-emitted {nodes} nodes of about 960"
        );
        assert!(cells <= 2_000, "a twinkle rastered {cells} cells of 14,941");
        assert!(bytes <= 4_096, "a twinkle wrote {bytes} bytes");
    });
}

#[test]
fn the_splash_never_asks_to_wake_sooner_than_a_beat() {
    Runtime::scope("splash-wake-floor", || {
        let (app, _host, _screen) = super::tests::splash_screen(SIZE.0, SIZE.1);
        let beat = crate::limits::limits().animation();
        for seconds in [0.0, 1.5, 30.0] {
            let wake = super::next_change(&app, seconds);
            assert!(wake >= beat, "woke after {wake:?} at {seconds}s");
        }
    });
}

#[test]
fn a_busy_splash_ticks_the_app_clock() {
    Runtime::scope("splash-busy-ticks", || {
        let (app, mut host, mut screen) = super::tests::splash_screen(SIZE.0, SIZE.1);
        app.update(crate::tui::Msg::Busy(Some(crate::tui::Activity::Turn)));
        let mut router = sigil::events::EventRouter::new();
        let beat = crate::limits::limits().animation();
        for _ in 0..40 {
            screen.frame(&mut host, &mut router, [], SIZE).unwrap();
            if app.ticks.peek() > 0 {
                break;
            }
            std::thread::park_timeout(beat);
        }
        assert!(app.ticks.peek() > 0, "a busy splash never ticked");
    });
}

#[test]
fn the_splash_sky_shows_the_stars_of_its_clock() {
    Runtime::scope("splash-sky-clock", || {
        let (app, mut host, mut screen) = super::tests::splash_screen(SIZE.0, SIZE.1);
        let per_beat = crate::limits::limits().animation().as_secs_f64().max(0.001);
        let window = super::window_area(&app, SIZE.0, SIZE.1);
        let outside = |x: u16, y: u16| {
            x < window[0]
                || y < window[1]
                || x >= window[0] + window[2]
                || y >= window[1] + window[3]
        };
        let stars = |beat: f64| {
            super::super::space::moving(SIZE.0, SIZE.1, beat, app.sky)
                .into_iter()
                .filter(|spot| outside(spot.x, spot.y))
                .map(|spot| (spot.x, spot.y, spot.cell.text.to_string()))
                .collect::<Vec<_>>()
        };
        let still = stars(0.0);
        let (beat, moved) = (1..2_000_u32)
            .map(f64::from)
            .map(|beat| {
                let moved: Vec<_> = stars(beat)
                    .into_iter()
                    .filter(|spot| !still.contains(spot))
                    .collect();
                (beat, moved)
            })
            .find(|(_, moved)| !moved.is_empty())
            .expect("a sky that changes after its first beat");
        app.sky_clock
            .set(Some(sigil::prelude::signal(beat * per_beat)));
        let mut router = sigil::events::EventRouter::new();
        screen.frame(&mut host, &mut router, [], SIZE).unwrap();
        let grid = screen
            .frame(&mut host, &mut router, [], SIZE)
            .unwrap()
            .clone();
        for (x, y, text) in moved {
            assert_eq!(
                grid.get(usize::from(x), usize::from(y))
                    .map(|cell| cell.text.to_string()),
                Some(text),
                "star at {x},{y} on beat {beat}"
            );
        }
    });
}
