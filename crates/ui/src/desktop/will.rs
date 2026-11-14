mod body;
mod mood;
mod rig;
mod shape;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use sigil::prelude::{
    MouseButton, NodeBuilder, Position, Role, Signal, Theme, Widget, container, interval, signal,
};
use sigil::reactive::{derived, untrack};
use sigil::widgets::MenuItem;

pub(super) use mood::Mood;

use self::body::Body;
use self::mood::{Mind, Shot};
use super::App;
use super::settings::Page;
use super::style::{WILL, will_ink};

/// How often a moving Will redraws.
const PERIOD: Duration = Duration::from_millis(33);
/// How often a still Will checks whether a reaction has run out.
const SETTLE: Duration = Duration::from_millis(250);
/// The closest Will comes to the column's edges, in points.
const EDGE: f32 = 8.0;
/// How far the pointer moves before a press becomes a drag, in points.
const SLOP: f32 = 3.0;
/// How much larger than his box Will's drawing is, so embers, sparks and the dot have room.
const ROOM: f32 = 1.5;
/// How far down his box the middle of his body sits.
const MIDDLE: f32 = 0.6;
const SETTINGS: &str = "Will settings…";
const HIDE: &str = "Hide Will";

/// The last pointer position over the main column, in window points.
pub(super) type Pointer = Rc<Cell<Option<(f32, f32)>>>;

/// Where a drawn Will takes his mood from.
#[derive(Clone)]
enum Source {
    Live(Mind),
    Fixed(Signal<Mood>),
}

impl Source {
    fn mood(&self, app: &App) -> Mood {
        match self {
            Self::Live(mind) => mind.mood(app),
            Self::Fixed(mood) => mood.get(),
        }
    }

    fn shots(&self) -> Vec<(Shot, Instant)> {
        match self {
            Self::Live(mind) => mind.shots(),
            Self::Fixed(_) => Vec::new(),
        }
    }

    fn look(&self, app: &App) -> (f32, f32) {
        match self {
            Self::Live(mind) => mind.look(app),
            Self::Fixed(_) => (0.0, 0.0),
        }
    }
}

/// Records where the pointer is over `node` so Will's eyes can follow it.
pub(super) fn track(node: NodeBuilder, pointer: &Pointer) -> NodeBuilder {
    let pointer = Rc::clone(pointer);
    node.pointer_move_capture(move |event| {
        let at = event.window_position;
        pointer.set(Some((at.x, at.y)));
    })
}

/// Will over the main column while the Will setting is on; the user drags him anywhere.
pub(super) fn layer(app: &App, pointer: &Pointer) -> NodeBuilder {
    let model = app.model;
    let live = signal(None::<(f32, f32)>);
    let room = room(app);
    let spot = move |axis: usize| {
        derived(move || {
            let saved = model.with(|model| model.prefs.will_spot[axis]) as f32;
            let held = live.get().map(|at: (f32, f32)| [at.0, at.1][axis]);
            let most = room();
            fit(held.unwrap_or(saved), [most.0, most.1][axis])
        })
    };
    let (right, bottom) = (spot(0), spot(1));
    let shown = derived(move || model.with(|model| model.prefs.will));
    let place = Rc::new(move || {
        let view = untrack(|| sigil::app::viewport_size().get());
        let (right, bottom) = untrack(|| (right.get(), bottom.get()));
        (
            view.width - right - WILL / 2.0,
            view.height - bottom - WILL * (1.0 - MIDDLE),
            WILL,
        )
    });
    let mind = Mind::watch(app, Rc::clone(pointer), place);
    let app = app.clone();
    let body = container()
        .shrink(0.0)
        .hittable(false)
        .children_reactive(move || {
            if shown.get() {
                vec![untrack(|| grip(&app, &mind, live, room))]
            } else {
                Vec::new()
            }
        });
    let beside = spacer().drive(sigil::animation::Property::Width, right);
    let below = spacer().drive(sigil::animation::Property::Height, bottom);
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .position(Position::Absolute)
        .inset_right(0.0)
        .inset_bottom(0.0)
        .hittable(false)
        .child(
            sigil::prelude::row()
                .gap(sigil::prelude::Space::None)
                .align_items(sigil::prelude::AlignItems::Stretch)
                .hittable(false)
                .child(body)
                .child(beside),
        )
        .child(below)
}

/// An empty box that only takes up room, so Will sits away from the column's corner.
fn spacer() -> NodeBuilder {
    container().shrink(0.0).hittable(false)
}

/// Will drawn `size` points square, still and idle, for Settings.
pub(super) fn still(app: &App, size: f32) -> NodeBuilder {
    art(app, size, false, Source::Fixed(signal(Mood::Idle)))
}

/// Will drawn `size` points square in `mood`, moving while Motion is on.
pub(super) fn preview(app: &App, size: f32, mood: Signal<Mood>) -> NodeBuilder {
    let moving = untrack(|| app.model.with(|model| model.prefs.animations));
    art(app, size, moving, Source::Fixed(mood))
}

/// The largest right and bottom offsets that keep Will inside the main column.
fn room(app: &App) -> impl Fn() -> (f32, f32) + Copy + 'static {
    let sidebar = app.window.sidebar;
    move || {
        let view = sigil::app::viewport_size().get();
        let side = if sidebar.get() && view.width >= super::sidebar::ROOMY {
            super::sidebar::WIDTH
        } else {
            0.0
        };
        (view.width - side - WILL - EDGE, view.height - WILL - EDGE)
    }
}

fn fit(offset: f32, most: f32) -> f32 {
    offset.min(most).max(EDGE)
}

fn grip(
    app: &App,
    mind: &Mind,
    live: Signal<Option<(f32, f32)>>,
    room: impl Fn() -> (f32, f32) + Copy + 'static,
) -> NodeBuilder {
    let moving = untrack(|| app.model.with(|model| model.prefs.animations));
    let held = Rc::new(Cell::new(None::<(f32, f32, f32, f32)>));
    let (press, drag, model, save) = (Rc::clone(&held), Rc::clone(&held), app.model, app.clone());
    let (enter, poke, watched) = (mind.clone(), mind.clone(), app.clone());
    let touched = watched.clone();
    let body = container()
        .role(Role::Image)
        .label("Will")
        .width(WILL)
        .height(WILL)
        .child(art(app, WILL, moving, Source::Live(mind.clone())))
        .pointer_enter(move |_| enter.enter(&watched))
        .pointer_down(move |event| {
            if event.button != Some(MouseButton::Left) {
                return;
            }
            let [right, bottom] = untrack(|| model.with(|model| model.prefs.will_spot));
            let (most_right, most_bottom) = untrack(room);
            let at = event.window_position;
            press.set(Some((
                at.x,
                at.y,
                fit(right as f32, most_right),
                fit(bottom as f32, most_bottom),
            )));
            event.capture_pointer();
            event.stop_propagation();
        })
        .pointer_move(move |event| {
            let Some((x, y, right, bottom)) = drag.get() else {
                return;
            };
            let at = event.window_position;
            if live.peek().is_none() && (at.x - x).hypot(at.y - y) < SLOP {
                return;
            }
            let (most_right, most_bottom) = untrack(room);
            live.set(Some((
                fit(right - (at.x - x), most_right),
                fit(bottom - (at.y - y), most_bottom),
            )));
            event.stop_propagation();
        })
        .pointer_up(move |event| {
            if held.take().is_none() {
                return;
            }
            if let Some((right, bottom)) = live.peek() {
                save.flip(|prefs| prefs.will_spot = [right.round() as i32, bottom.round() as i32]);
            } else {
                poke.poke(&touched);
            }
            live.set(None);
            event.stop_propagation();
        });
    let menu = app.clone();
    container()
        .width(WILL)
        .height(WILL)
        .shrink(0.0)
        .child(sigil::widgets::context_menu(body, move || {
            let (open, hide) = (menu.clone(), menu.clone());
            vec![
                MenuItem::new(SETTINGS).activate(move || super::settings::open(&open, Page::Will)),
                MenuItem::new(HIDE).activate(move || hide.flip(|prefs| prefs.will = false)),
            ]
        }))
}

fn art(app: &App, size: f32, moving: bool, source: Source) -> NodeBuilder {
    let (model, appearance) = (app.model, app.appearance);
    let frame = signal(0_u64);
    let period = if moving { PERIOD } else { SETTLE };
    interval(period, move || frame.update(|at| *at = at.wrapping_add(1)));
    let (watch, seen) = (app.clone(), source.clone());
    let look = derived(move || {
        _ = frame.get();
        let light = u64::from(appearance.get().light());
        let tint = model.with(|model| model.prefs.will_tint) as u64;
        light << 16 | tint << 8 | seen.mood(&watch) as u64
    });
    let body = RefCell::new(Body::new(Mood::Idle));
    let (theme, app) = (Theme::dark(), app.clone());
    let drawn = size * ROOM;
    let paint = sigil::widgets::canvas(move |scene, area| {
        let (mood, tint, shots, eyes) = untrack(|| {
            (
                source.mood(&app),
                model.with(|model| model.prefs.will_tint),
                source.shots(),
                source.look(&app),
            )
        });
        let frame = body.borrow_mut().step(mood, moving, &shots, eyes);
        let ink = will_ink(tint, appearance.peek().light());
        shape::draw(scene, area, &frame, &ink, &theme);
    })
    .redraw_on(if moving { frame } else { look })
    .width(drawn)
    .height(drawn);
    let paint = container()
        .hittable(false)
        .position(Position::Absolute)
        .inset_left((size - drawn) / 2.0)
        .inset_top((size - drawn) / 2.0)
        .width(drawn)
        .height(drawn)
        .child(paint);
    container()
        .position(Position::Relative)
        .width(size)
        .height(size)
        .shrink(0.0)
        .child(paint)
}
