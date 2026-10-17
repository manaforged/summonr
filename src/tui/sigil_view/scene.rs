use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

pub(super) use super::app::App;

use super::canvas::{Cell, blend, cell as metrics, ink, palette, rgb, shown};
use super::model::Model;
use sigil::layout::Position;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Paint;
use sigil::prelude::Rgba8;
use sigil::prelude::container;

pub(in crate::tui::sigil_view) use input::window_area;

mod logo;
mod mark;
pub(in crate::tui::sigil_view) use logo::{compose, placed};
pub(in crate::tui::sigil_view) use mark::Mark;
pub(in crate::tui::sigil_view) use space::Plate;
pub(in crate::tui::sigil_view) use space::{GLASS, PLATE, SCRIM, VOID, moving};

mod galaxy;
mod input;
mod space;
mod stars;

pub(in crate::tui::sigil_view) fn fresh_sky() -> u64 {
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    clock ^ u64::from(std::process::id()).rotate_left(32)
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let stars = app.clone();
    let painter = app.clone();
    let shape = app.clone();
    let beats_per_second = 1.0 / crate::limits::limits().animation().as_secs_f64().max(0.001);
    let sky = sigil::reactive::derived(move || {
        let (cols, rows) = stars.viewport.get();
        drop(stars.theme_name.get());
        let beats = stars
            .sky_clock
            .get()
            .map_or(0.0, |clock| clock.get() * beats_per_second);
        moving(cols, rows, beats, stars.sky)
    });
    let size = sigil::reactive::derived(move || {
        let (cols, rows) = shape.viewport.get();
        (u64::from(cols) << 16) | u64::from(rows)
    });
    let mut beats = 0_u64;
    let twinkle = sigil::reactive::derived(move || {
        sky.with(|_| ());
        beats = beats.wrapping_add(1);
        beats
    });
    let card = card(app);
    container()
        .bg(VOID)
        .layout(fill(LayoutStyle::flex_column()))
        .child(
            sigil::widgets::canvas(move |scene, area| {
                galaxy::paint_sky(scene, area, painter.viewport.peek(), painter.sky);
            })
            .redraw_on(size)
            .layout(fill(LayoutStyle::block())),
        )
        .child(
            sigil::widgets::canvas(move |scene, area| {
                sigil::reactive::untrack(|| {
                    sky.with(|spots| space::paint_spots(scene, area, spots));
                });
            })
            .redraw_on(twinkle)
            .layout(fill(LayoutStyle::block())),
        )
        .child(card_rows(card))
}

fn card(app: App) -> sigil::prelude::Signal<Plate> {
    let reader = app.clone();
    let inputs = sigil::reactive::derived(move || {
        let model = reader.model.get();
        (logo::facts(&model, &reader), model.prefs.animations)
    });
    let painter = app.clone();
    let mut generation = 0_u64;
    let base = sigil::reactive::derived(move || {
        let (cols, rows) = painter.viewport.get();
        drop(painter.theme_name.get());
        let _backdrop = painter.backdrop.get();
        let _draft = painter.draft.get();
        let _since = painter.splash_since.get();
        inputs.with(|_| ());
        if !painter.splash_revealed.get() {
            let _reveal = painter.ticks.get();
        }
        generation = generation.wrapping_add(1);
        (
            generation,
            sigil::reactive::untrack(|| compose(cols, rows, &painter)),
        )
    });
    let mut seen = None;
    let mut work = Plate::default();
    let mut lit: Vec<[u16; 2]> = Vec::new();
    sigil::reactive::derived(move || {
        if app.splash.get() && inputs.with(|(_, animations)| *animations) {
            let _orbit = app.ticks.get();
        }
        base.with(|(generation, composed)| {
            if seen == Some(*generation) {
                for at in lit.drain(..) {
                    if let (Some(cell), Some(still)) = (work.cell(at), composed.plate.at(at)) {
                        cell.clone_from(still);
                    }
                }
            } else {
                seen = Some(*generation);
                lit.clear();
                work.clone_from(&composed.plate);
            }
            if let Some(shimmer) = &composed.shimmer {
                shimmer.apply(&mut work, logo::shimmer_clock(&app), &mut lit);
            }
        });
        work.clone()
    })
}

fn card_rows(card: sigil::prelude::Signal<Plate>) -> NodeBuilder {
    let count = sigil::reactive::derived(move || card.with(|plate| plate.area[3]));
    container()
        .layout(fill(LayoutStyle::block()))
        .children_reactive(move || {
            (0..count.get())
                .map(|index| {
                    let row = sigil::reactive::derived(move || card.with(|plate| plate.row(index)));
                    let frame = sigil::reactive::derived(move || {
                        row.with(|row| {
                            row.as_ref()
                                .map(|(x, y, cells)| [*x, *y, cells.len() as u16])
                        })
                    });
                    container()
                        .layout(fill(LayoutStyle::block()))
                        .children_reactive(move || frame.get().map(|frame| placed(frame, row)))
                })
                .collect::<Vec<_>>()
        })
}

fn fill(style: LayoutStyle) -> LayoutStyle {
    style
        .position(Position::Absolute)
        .inset(0.0)
        .fill_width()
        .fill_height()
        .gap(0.0)
}
