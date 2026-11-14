use std::rc::Rc;

use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

pub(super) use super::app::App;

use super::canvas::{Cell, blend, cell as metrics, ink, rgb, shown};
use super::model::Model;
use super::theme::palette;
use sigil::prelude::Color;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Paint;
use sigil::prelude::Position;
use sigil::prelude::Space;
use sigil::prelude::Widget;
use sigil::prelude::container;

pub(crate) use input::window_area;

mod lockup;
mod logo;
mod mark;
mod sprite;
pub(crate) use lockup::{ground, paned};
pub(crate) use logo::{Line, compose, placed, reveal_ms};
pub(crate) use mark::Mark;
pub(crate) use space::Plate;
pub(crate) use space::moving;
pub(crate) use stage::stage;
#[cfg(feature = "gui")]
pub(crate) use wash::DAY;

mod galaxy;
mod input;
mod space;
mod stage;
mod stars;
mod wash;

pub(crate) fn sky_seed() -> u64 {
    static SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *SEED.get_or_init(fresh_sky)
}

pub(crate) fn fresh_sky() -> u64 {
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    clock ^ u64::from(std::process::id()).rotate_left(32)
}

pub(crate) fn layer(app: App) -> NodeBuilder {
    let picked = app.clone();
    let splash = sigil::reactive::derived(move || picked.model.with(|model| model.prefs.splash));
    let card = card(app.clone());
    fill(container()).children_reactive(move || {
        let splash = splash.get();
        vec![sigil::reactive::untrack(|| backdrop(app.clone(), splash, card))]
    })
}

fn backdrop(
    app: App,
    splash: summonr_agent::prefs::Splash,
    card: sigil::prelude::Signal<Rc<[Line]>>,
) -> NodeBuilder {
    use summonr_agent::prefs::Splash;
    let root = fill(sigil::prelude::column());
    if matches!(splash, Splash::Plain | Splash::Motes) {
        return root.background(palette().base).child(card_rows(card));
    }
    let stars = app.clone();
    let painter = app.clone();
    let shape = app.clone();
    let beats_per_second = 1.0
        / summonr_agent::limits::limits()
            .animation()
            .as_secs_f64()
            .max(0.001);
    let sky = sigil::reactive::derived(move || {
        let (cols, rows) = stars.viewport.get();
        let _appearance = stars.appearance.get();
        let beats = sky_seconds(&stars) * beats_per_second;
        moving(cols, rows, beats, stars.sky.peek())
    });
    let size = sigil::reactive::derived(move || {
        let (cols, rows) = shape.viewport.get();
        let base = shape.appearance.get().base;
        let tone = u64::from(u32::from_be_bytes([base.r, base.g, base.b, 0]));
        (tone << 32) | (u64::from(cols) << 16) | u64::from(rows)
    });
    let mut beats = 0_u64;
    let twinkle = sigil::reactive::derived(move || {
        sky.with(|_| ());
        beats = beats.wrapping_add(1);
        beats
    });
    let root = root
        .background(stage().void)
        .child(fill(
            sigil::widgets::canvas(move |scene, area| {
                galaxy::paint_sky(scene, area, painter.viewport.peek(), painter.sky.peek());
            })
            .redraw_on(size),
        ))
        .child(fill(
            sigil::widgets::canvas(move |scene, area| {
                sigil::reactive::untrack(|| {
                    sky.with(|spots| space::paint_spots(scene, area, spots));
                });
            })
            .redraw_on(twinkle),
        ));
    root.child(card_rows(card))
}

fn sky_seconds(app: &App) -> f64 {
    app.sky_clock.get().map_or(0.0, |clock| clock.get())
}

pub(crate) fn next_change(app: &App, seconds: f64) -> std::time::Duration {
    let animation = summonr_agent::limits::limits().animation();
    if !app.splash_revealed.peek() {
        return animation;
    }
    let per_beat = animation.as_secs_f64().max(0.001);
    let (cols, rows) = app.viewport.get();
    let sky = space::sky_wait(cols, rows, seconds / per_beat, app.sky.peek())
        .map(|beats| beats * per_beat);
    let clock = logo::shimmer_clock(app);
    let shimmer = if lockup::drawn(app.model.with(|model| model.prefs.splash)) {
        lockup::frame_wait(clock)
    } else {
        mark::shimmer_wait(clock)
    } as f64
        / 1000.0;
    let wait = sky.map_or(shimmer, |sky| sky.min(shimmer));
    let ticks = (wait / per_beat).ceil().max(1.0);
    std::time::Duration::try_from_secs_f64(ticks * per_beat).map_or(animation, |wake| {
        wake + summonr_agent::limits::limits().wake_margin()
    })
}

fn card(app: App) -> sigil::prelude::Signal<Rc<[Line]>> {
    let reader = app.clone();
    let inputs = sigil::reactive::derived(move || {
        let model = reader.model.get();
        (logo::facts(&model, &reader), model.prefs.animations)
    });
    let typed = app.clone();
    let blank = sigil::reactive::derived(move || typed.draft.with(String::is_empty));
    let painter = app.clone();
    let mut generation = 0_u64;
    let base = sigil::reactive::derived(move || {
        let (cols, rows) = painter.viewport.get();
        let _appearance = painter.appearance.get();
        let _blank = blank.get();
        let _since = painter.splash_since.get();
        inputs.with(|_| ());
        if !painter.splash_revealed.get() {
            let _reveal = sky_seconds(&painter);
        }
        generation = generation.wrapping_add(1);
        (
            generation,
            sigil::reactive::untrack(|| {
                compose(cols, rows, &painter, logo::shimmer_clock(&painter))
            }),
        )
    });
    let mut painted = Painted::default();
    sigil::reactive::derived(move || {
        if app.splash.get() && inputs.with(|(_, animations)| *animations) {
            let _orbit = sky_seconds(&app);
        }
        base.with(|(generation, composed)| {
            let mut dirty = painted.settle(*generation, composed);
            if let Some(shimmer) = &composed.shimmer {
                shimmer.apply(
                    &mut painted.work,
                    logo::shimmer_clock(&app),
                    &mut painted.lit,
                );
                dirty.extend(painted.lit.iter().map(|at| at[1]));
            }
            if let Some(figure) = &composed.will {
                figure.apply(
                    &mut painted.work,
                    &composed.plate,
                    logo::shimmer_clock(&app),
                    &mut painted.lit,
                );
                dirty.extend(painted.lit.iter().map(|at| at[1]));
            }
            painted.refresh(dirty);
        });
        Rc::<[Line]>::from(painted.lines.as_slice())
    })
}

#[derive(Default)]
struct Painted {
    seen: Option<u64>,
    work: Plate,
    lit: Vec<[u16; 2]>,
    lines: Vec<Line>,
}

impl Painted {
    fn settle(&mut self, generation: u64, composed: &logo::Composed) -> Vec<u16> {
        if self.seen == Some(generation) {
            let mut dirty = Vec::new();
            for at in self.lit.drain(..) {
                if let (Some(cell), Some(still)) = (self.work.cell(at), composed.plate.at(at)) {
                    cell.clone_from(still);
                    dirty.push(at[1]);
                }
            }
            return dirty;
        }
        self.seen = Some(generation);
        self.lit.clear();
        self.work.clone_from(&composed.plate);
        let old = std::mem::take(&mut self.lines);
        let work = &self.work;
        self.lines = (0..work.area[3])
            .filter_map(|index| work.row(index))
            .enumerate()
            .map(|(index, row)| match old.get(index) {
                Some(line) if *line.0 == row => line.clone(),
                _ => Line(Rc::new(row)),
            })
            .collect();
        Vec::new()
    }

    fn refresh(&mut self, mut dirty: Vec<u16>) {
        dirty.sort_unstable();
        dirty.dedup();
        for y in dirty {
            let Some(index) = y.checked_sub(self.work.area[1]) else {
                continue;
            };
            if let (Some(line), Some(cells)) = (
                self.lines.get_mut(usize::from(index)),
                self.work.cells_of(index),
            ) && line.0.2.as_slice() != cells
            {
                *line = Line(Rc::new((line.0.0, line.0.1, cells.to_vec())));
            }
        }
    }
}

fn card_rows(card: sigil::prelude::Signal<Rc<[Line]>>) -> NodeBuilder {
    let count = sigil::reactive::derived(move || card.with(|lines| lines.len()));
    fill(container()).children_reactive(move || {
        (0..count.get())
            .map(|index| {
                let row =
                    sigil::reactive::derived(move || card.with(|lines| lines.get(index).cloned()));
                let frame = sigil::reactive::derived(move || {
                    row.with(|row| {
                        row.as_ref()
                            .map(|Line(row)| [row.0, row.1, row.2.len() as u16])
                    })
                });
                fill(container())
                    .children_reactive(move || frame.get().map(|frame| placed(frame, row)))
            })
            .collect::<Vec<_>>()
    })
}

fn fill<W: Widget>(node: W) -> W {
    node.position(Position::Absolute)
        .inset(0.0)
        .fill_width()
        .fill_height()
        .gap(Space::None)
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
