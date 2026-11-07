use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use sigil::geometry::Rect;
use sigil::prelude::{LayoutStyle, NodeBuilder, Rgba8, Role, Theme, Widget, container, interval, signal};
use sigil::scene::SceneBuilder;
use sigil::widgets::PathNode;

use super::style::{MARK, WILL_DARK, WILL_LIGHT};

const PERIOD: Duration = Duration::from_millis(50);
const FLICKER: f32 = 1.1;
const SWAY: f32 = 2.1;
const STRETCH: f32 = 0.025;
const POSE: (f32, f32) = (1.03, 1.08);
const BASE: (f32, f32) = (8.0, 15.0);

static CLOCK: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Copy)]
enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Curve([f32; 6]),
    Close,
}

const FLAME: [Seg; 9] = [
    Seg::Move(7.3, 1.8),
    Seg::Curve([7.8, 3.6, 9.6, 4.2, 11.0, 5.2]),
    Seg::Curve([12.2, 6.1, 13.0, 7.4, 13.0, 9.0]),
    Seg::Curve([13.0, 11.76, 10.76, 14.0, 8.0, 14.0]),
    Seg::Curve([5.24, 14.0, 3.0, 11.76, 3.0, 9.0]),
    Seg::Curve([3.0, 7.6, 3.5, 6.6, 4.2, 5.8]),
    Seg::Curve([4.6, 6.6, 5.2, 6.8, 5.8, 6.7]),
    Seg::Curve([5.5, 5.0, 6.1, 3.2, 7.3, 1.8]),
    Seg::Close,
];

const CORE: [Seg; 6] = [
    Seg::Move(11.1, 10.0),
    Seg::Curve([11.1, 11.712, 9.712, 13.1, 8.0, 13.1]),
    Seg::Curve([6.288, 13.1, 4.9, 11.712, 4.9, 10.0]),
    Seg::Curve([4.9, 8.288, 6.288, 6.9, 8.0, 6.9]),
    Seg::Curve([9.712, 6.9, 11.1, 8.288, 11.1, 10.0]),
    Seg::Close,
];

const EYES: [Seg; 10] = [
    Seg::Move(5.9, 10.0),
    Seg::Line(7.3, 10.0),
    Seg::Line(7.3, 11.2),
    Seg::Line(5.9, 11.2),
    Seg::Close,
    Seg::Move(8.7, 10.0),
    Seg::Line(10.1, 10.0),
    Seg::Line(10.1, 11.2),
    Seg::Line(8.7, 11.2),
    Seg::Close,
];

/// Will in the working row's mark slot: a flame that flickers while the agent works.
pub(super) fn mark(moving: bool) -> NodeBuilder {
    let frame = signal(0_u64);
    if moving {
        interval(PERIOD, move || frame.update(|at| *at = at.wrapping_add(1)));
    }
    let start = *CLOCK.get_or_init(Instant::now);
    let theme = Theme::dark();
    let paint = sigil::widgets::canvas(move |scene, area| {
        let seconds = if moving { start.elapsed().as_secs_f32() } else { 0.0 };
        draw(scene, area, seconds, &theme);
    })
    .redraw_on(frame)
    .layout(LayoutStyle::block().width(MARK).height(MARK));
    container()
        .role(Role::ProgressIndicator)
        .label("Working")
        .layout(LayoutStyle::block().width(MARK).height(MARK))
        .child(paint)
}

fn draw(scene: &mut SceneBuilder, area: Rect, seconds: f32, theme: &Theme) {
    let [edge, core, eyes] = if crate::theme::palette().light() { WILL_LIGHT } else { WILL_DARK };
    let wave = (seconds * TAU / FLICKER).sin();
    let (sin, cos) = (wave * SWAY).to_radians().sin_cos();
    let (sx, sy) = (POSE.0, POSE.1 * (1.0 + STRETCH * (wave + 1.0) / 2.0));
    let place = move |x: f32, y: f32| {
        let (dx, dy) = ((x - BASE.0) * sx, (y - BASE.1) * sy);
        (BASE.0 + dx * cos - dy * sin, BASE.1 + dx * sin + dy * cos)
    };
    for (segs, color) in [(&FLAME[..], edge), (&CORE[..], core), (&EYES[..], eyes)] {
        outline(segs, &place, color).draw(scene, area, theme);
    }
}

fn outline(segs: &[Seg], place: &impl Fn(f32, f32) -> (f32, f32), color: Rgba8) -> PathNode {
    segs.iter()
        .fold(sigil::widgets::path(16, 16), |node, seg| match *seg {
            Seg::Move(x, y) => {
                let (x, y) = place(x, y);
                node.move_to(x, y)
            }
            Seg::Line(x, y) => {
                let (x, y) = place(x, y);
                node.line_to(x, y)
            }
            Seg::Curve([ax, ay, bx, by, x, y]) => {
                let ((ax, ay), (bx, by), (x, y)) = (place(ax, ay), place(bx, by), place(x, y));
                node.cubic_to(ax, ay, bx, by, x, y)
            }
            Seg::Close => node.close(),
        })
        .fill(color)
}
