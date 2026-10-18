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
