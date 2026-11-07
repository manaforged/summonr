mod mood;
mod shape;

use std::cell::{Cell, RefCell};
use std::f32::consts::TAU;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use sigil::prelude::{
    LayoutStyle, MouseButton, NodeBuilder, Position, Role, Signal, Theme, Widget, container,
    interval, signal,
};
use sigil::reactive::{derived, untrack};
use sigil::widgets::MenuItem;

pub(super) use mood::Mood;

use self::mood::{Mind, Pose};
use self::shape::Frame;
use super::App;
use super::settings::Page;
use super::style::{WILL, will_colors};

/// How often a moving Will redraws.
const PERIOD: Duration = Duration::from_millis(33);
/// How often a still Will checks whether a reaction has run out.
const SETTLE: Duration = Duration::from_millis(250);
/// The closest Will comes to the column's edges, in points.
const EDGE: f32 = 8.0;
/// How far the pointer moves before a press becomes a drag, in points.
const SLOP: f32 = 3.0;
/// Spring stiffness and damping for pose changes: a short ease with a little overshoot.
const SPRING: (f32, f32) = (180.0, 14.0);
/// How long a blink keeps the eyes shut, and the shortest and longest gaps between blinks.
const BLINK: (Duration, f32, f32) = (Duration::from_millis(110), 2.4, 6.0);
/// How long one keystroke lifts Will, and by how much on the 16 grid.
const TICK: (Duration, f32) = (Duration::from_millis(70), -0.7);
const SETTINGS: &str = "Will settings…";
const HIDE: &str = "Hide Will";

static CLOCK: OnceLock<Instant> = OnceLock::new();

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

    fn typed_at(&self) -> Option<Instant> {
        match self {
            Self::Live(mind) => *mind.typed_at.borrow(),
            Self::Fixed(_) => None,
        }
    }
}

/// Will over the main column while the Will setting is on; the user drags him anywhere.
pub(super) fn layer(app: &App) -> NodeBuilder {
    let model = app.model;
    let live = signal(None::<(f32, f32)>);
    let room = room(app);
    let spot = move |axis: usize| {
        derived(move || {
            let saved = model.with(|model| model.prefs.will_spot[axis]) as f32;
            let held = live.get().map(|at: (f32, f32)| [at.0, at.1][axis]);
            let most = room();
            -fit(held.unwrap_or(saved), [most.0, most.1][axis])
        })
    };
    let shown = derived(move || model.with(|model| model.prefs.will));
    let mind = Mind::watch(app);
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
                vec![untrack(|| grip(&app, &mind, live, room))]
            } else {
                Vec::new()
            }
        })
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
    let (enter, leave) = (mind.hover, mind.hover);
    let body = container()
        .role(Role::Image)
        .label("Will")
        .layout(LayoutStyle::block().width(WILL).height(WILL))
        .child(art(app, WILL, moving, Source::Live(mind.clone())))
        .pointer_enter(move |_| enter.set(true))
        .pointer_leave(move |_| leave.set(false))
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
            }
            live.set(None);
            event.stop_propagation();
        });
    let menu = app.clone();
    container()
        .layout(LayoutStyle::block().width(WILL).height(WILL).shrink(0.0))
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
        let _ = frame.get();
        let light = u64::from(appearance.get().light());
        let tint = model.with(|model| model.prefs.will_tint) as u64;
        light << 16 | tint << 8 | seen.mood(&watch) as u64
    });
    let body = RefCell::new(Body::new());
    let (theme, app) = (Theme::dark(), app.clone());
    let paint = sigil::widgets::canvas(move |scene, area| {
        let (mood, tint) =
            untrack(|| (source.mood(&app), model.with(|model| model.prefs.will_tint)));
        let pose = mood.pose();
        let drawn = body.borrow_mut().step(&pose, moving, source.typed_at());
        let colors = will_colors(tint, appearance.peek().light());
        shape::draw(scene, area, &drawn, colors, &theme);
    })
    .redraw_on(if moving { frame } else { look })
    .layout(LayoutStyle::block().width(size).height(size));
    container()
        .layout(LayoutStyle::block().width(size).height(size).shrink(0.0))
        .child(paint)
}

/// Will's moving body: each pose value springs toward its target, and the flame flickers.
struct Body {
    at: [f32; 7],
    speed: [f32; 7],
    phase: f32,
    last: Option<Instant>,
    blink: Instant,
}

impl Body {
    fn new() -> Self {
        Self {
            at: target(&Mood::Idle.pose()),
            speed: [0.0; 7],
            phase: 0.0,
            last: None,
            blink: Instant::now() + Duration::from_secs_f32(BLINK.1),
        }
    }

    fn step(&mut self, pose: &Pose, moving: bool, typed_at: Option<Instant>) -> Frame {
        let goal = target(pose);
        let now = Instant::now();
        let mut blink = 1.0;
        let mut tick = 0.0;
        if moving {
            let dt = self
                .last
                .map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.1));
            self.spring(&goal, dt);
            self.phase = (self.phase + dt * TAU / pose.flicker) % TAU;
            if now >= self.blink + BLINK.0 {
                self.blink = now + Duration::from_secs_f32(gap(now));
            }
            if now >= self.blink {
                blink = 0.1;
            }
            if typed_at.is_some_and(|at| now.duration_since(at) < TICK.0) {
                tick = TICK.1;
            }
        } else {
            self.at = goal;
            self.speed = [0.0; 7];
            self.phase = 0.0;
        }
        self.last = Some(now);
        let wave = self.phase.sin();
        let [sx, sy, tilt, lift, fade, lx, ly] = self.at;
        Frame {
            scale: (sx, sy * (1.0 + pose.stretch * (wave + 1.0) / 2.0)),
            tilt: tilt + wave * pose.sway,
            lift: lift + tick,
            fade,
            eyes: pose.eyes,
            look: (lx, ly),
            blink,
        }
    }

    fn spring(&mut self, goal: &[f32; 7], dt: f32) {
        const STEP: f32 = 1.0 / 120.0;
        let (stiff, damp) = SPRING;
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(STEP);
            for at in 0..7 {
                let pull = stiff * (goal[at] - self.at[at]) - damp * self.speed[at];
                self.speed[at] += pull * h;
                self.at[at] += self.speed[at] * h;
            }
            left -= h;
        }
    }
}

const fn target(pose: &Pose) -> [f32; 7] {
    [
        pose.scale.0,
        pose.scale.1,
        pose.tilt,
        pose.lift,
        pose.fade,
        pose.look.0,
        pose.look.1,
    ]
}

/// A gap before the next blink, spread between the shortest and longest from the clock.
fn gap(now: Instant) -> f32 {
    let start = *CLOCK.get_or_init(Instant::now);
    let spread = (now.duration_since(start).subsec_micros() % 1000) as f32 / 1000.0;
    BLINK.1 + (BLINK.2 - BLINK.1) * spread
}
