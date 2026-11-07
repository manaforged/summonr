use std::cell::Cell;
use std::f32::consts::TAU;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use sigil::geometry::Rect;
use sigil::prelude::{
    LayoutStyle, NodeBuilder, Position, Role, Signal, Theme, Widget, container, interval, signal,
};
use sigil::reactive::{derived, untrack};
use sigil::scene::SceneBuilder;
use sigil::widgets::PathNode;

use super::App;
use super::style::{WILL, will_colors};

const PERIOD: Duration = Duration::from_millis(50);
const BASE: (f32, f32) = (8.0, 15.0);
/// The closest Will comes to the column's right and bottom edges, in points.
const EDGE: f32 = 8.0;

static CLOCK: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Copy)]
enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Curve([f32; 6]),
    Close,
}

/// How Will holds himself in one state.
struct Pose {
    flicker: f32,
    sway: f32,
    stretch: f32,
    scale: (f32, f32),
    eyes: &'static [Seg],
}

const IDLE: Pose = Pose {
    flicker: 2.6,
    sway: 2.0,
    stretch: 0.02,
    scale: (1.0, 1.0),
    eyes: &OPEN,
};

const WORKING: Pose = Pose {
    flicker: 1.1,
    sway: 2.1,
    stretch: 0.025,
    scale: (1.03, 1.08),
    eyes: &FOCUS,
};

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

const OPEN: [Seg; 10] = [
    Seg::Move(6.0, 9.0),
    Seg::Line(7.2, 9.0),
    Seg::Line(7.2, 11.2),
    Seg::Line(6.0, 11.2),
    Seg::Close,
    Seg::Move(8.8, 9.0),
    Seg::Line(10.0, 9.0),
    Seg::Line(10.0, 11.2),
    Seg::Line(8.8, 11.2),
    Seg::Close,
];

const FOCUS: [Seg; 10] = [
    Seg::Move(5.85, 10.7),
    Seg::Line(7.35, 10.7),
    Seg::Line(7.35, 11.4),
    Seg::Line(5.85, 11.4),
    Seg::Close,
    Seg::Move(8.65, 10.7),
    Seg::Line(10.15, 10.7),
    Seg::Line(10.15, 11.4),
    Seg::Line(8.65, 11.4),
    Seg::Close,
];

/// Will over the main column while the Will setting is on; the user drags him anywhere.
pub(super) fn layer(app: &App) -> NodeBuilder {
    let model = app.model;
    let live = signal(None::<(f32, f32)>);
    let spot = move |axis: usize| {
        derived(move || {
            let saved = model.with(|model| model.prefs.will_spot[axis]) as f32;
            let held = live.get().map(|at: (f32, f32)| [at.0, at.1][axis]);
            -held.unwrap_or(saved)
        })
    };
    let shown = derived(move || model.with(|model| model.prefs.will));
    let app = app.clone();
    container()
        .layout(
            LayoutStyle::block()
                .position(Position::Absolute)
                .inset_right(0.0)
                .inset_bottom(0.0),
        )
        .animate_translate_x(spot(0))
        .animate_translate_y(spot(1))
        .children_reactive(move || {
            if shown.get() {
                vec![untrack(|| grip(&app, live))]
            } else {
                Vec::new()
            }
        })
}

/// Will drawn `size` points square, still, for Settings.
pub(super) fn still(app: &App, size: f32) -> NodeBuilder {
    art(app, size, false)
}

fn grip(app: &App, live: Signal<Option<(f32, f32)>>) -> NodeBuilder {
    let moving = untrack(|| app.model.with(|model| model.prefs.animations));
    let held = Rc::new(Cell::new(None::<(f32, f32, f32, f32)>));
    let (press, drag, model, save) = (Rc::clone(&held), Rc::clone(&held), app.model, app.clone());
    container()
        .role(Role::Image)
        .label("Will")
        .layout(LayoutStyle::block().width(WILL).height(WILL))
        .child(art(app, WILL, moving))
        .pointer_down(move |event| {
            let [right, bottom] = untrack(|| model.with(|model| model.prefs.will_spot));
            let at = event.window_position;
            press.set(Some((at.x, at.y, right as f32, bottom as f32)));
            event.capture_pointer();
            event.stop_propagation();
        })
        .pointer_move(move |event| {
            let Some((x, y, right, bottom)) = drag.get() else {
                return;
            };
            let at = event.window_position;
            live.set(Some((
                (right - (at.x - x)).max(EDGE),
                (bottom - (at.y - y)).max(EDGE),
            )));
            event.stop_propagation();
        })
        .pointer_up(move |event| {
            if held.take().is_none() {
                return;
            }
            if let Some((right, bottom)) = live.peek() {
                save.flip(|prefs| prefs.will_spot = [right.round() as i32, bottom.round() as i32]);
            }
            live.set(None);
            event.stop_propagation();
        })
}

fn art(app: &App, size: f32, moving: bool) -> NodeBuilder {
    let model = app.model;
    let look = derived(move || {
        model.with(|model| u64::from(model.busy_since.is_some()) << 8 | model.prefs.will_tint as u64)
    });
    let frame = signal(0_u64);
    if moving {
        interval(PERIOD, move || frame.update(|at| *at = at.wrapping_add(1)));
    }
    let start = *CLOCK.get_or_init(Instant::now);
    let theme = Theme::dark();
    let paint = sigil::widgets::canvas(move |scene, area| {
        let (working, tint) =
            untrack(|| model.with(|model| (model.busy_since.is_some(), model.prefs.will_tint)));
        let pose = if working { &WORKING } else { &IDLE };
        let seconds = if moving { start.elapsed().as_secs_f32() } else { 0.0 };
        let colors = will_colors(tint, crate::theme::palette().light());
        draw(scene, area, pose, seconds, colors, &theme);
    })
    .redraw_on(if moving { frame } else { look })
    .layout(LayoutStyle::block().width(size).height(size));
    container()
        .layout(LayoutStyle::block().width(size).height(size).shrink(0.0))
        .child(paint)
}

fn draw(
    scene: &mut SceneBuilder,
    area: Rect,
    pose: &Pose,
    seconds: f32,
    [edge, core, eyes]: [sigil::prelude::Rgba8; 3],
    theme: &Theme,
) {
    let wave = (seconds * TAU / pose.flicker).sin();
    let (sin, cos) = (wave * pose.sway).to_radians().sin_cos();
    let (sx, sy) = (
        pose.scale.0,
        pose.scale.1 * (1.0 + pose.stretch * (wave + 1.0) / 2.0),
    );
    let place = move |x: f32, y: f32| {
        let (dx, dy) = ((x - BASE.0) * sx, (y - BASE.1) * sy);
        (BASE.0 + dx * cos - dy * sin, BASE.1 + dx * sin + dy * cos)
    };
    for (segs, color) in [(&FLAME[..], edge), (&CORE[..], core), (pose.eyes, eyes)] {
        outline(segs, &place, color).draw(scene, area, theme);
    }
}

fn outline(
    segs: &[Seg],
    place: &impl Fn(f32, f32) -> (f32, f32),
    color: sigil::prelude::Rgba8,
) -> PathNode {
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
