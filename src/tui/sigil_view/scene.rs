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
    let mut frames = 0_u64;
    let version = sigil::reactive::derived(move || {
        let (cols, rows) = shape.viewport.get();
        sky.with(|_| ());
        frames = frames.wrapping_add(1);
        (frames << 32) | (u64::from(cols) << 16) | u64::from(rows)
    });
    let card = card(app);
    container()
        .bg(VOID)
        .layout(fill(LayoutStyle::flex_column()))
        .child(
            sigil::widgets::canvas(move |scene, area| {
                galaxy::paint_sky(scene, area, painter.viewport.peek(), painter.sky);
                sigil::reactive::untrack(|| {
                    sky.with(|spots| space::paint_spots(scene, area, spots));
                });
            })
            .redraw_on(version)
            .layout(fill(LayoutStyle::block())),
        )
        .child(card_rows(card))
}

type Row = (u16, u16, Vec<Cell>);

fn card(app: App) -> sigil::prelude::Signal<Vec<Row>> {
    let reader = app.clone();
    let inputs = sigil::reactive::derived(move || {
        let model = reader.model.get();
        (logo::facts(&model, &reader), model.prefs.animations)
    });
    let mut cells: Vec<Cell> = Vec::new();
    sigil::reactive::derived(move || {
        let (cols, rows) = app.viewport.get();
        drop(app.theme_name.get());
        let _backdrop = app.backdrop.get();
        let _draft = app.draft.get();
        let _since = app.splash_since.get();
        let animations = inputs.with(|(_, animations)| *animations);
        if !app.splash_revealed.get() || (app.splash.get() && animations) {
            let _reveal = app.ticks.get();
        }
        cells.clear();
        cells.resize(usize::from(cols) * usize::from(rows), Cell::blank());
        let [x, y, width, height] =
            sigil::reactive::untrack(|| compose(&mut cells, cols, rows, &app));
        (y..y + height)
            .map(|row| {
                let start = usize::from(row) * usize::from(cols) + usize::from(x);
                (x, row, cells[start..start + usize::from(width)].to_vec())
            })
            .collect()
    })
}

fn card_rows(card: sigil::prelude::Signal<Vec<Row>>) -> NodeBuilder {
    let count = sigil::reactive::derived(move || card.with(Vec::len));
    container()
        .layout(fill(LayoutStyle::block()))
        .children_reactive(move || {
            (0..count.get())
                .map(|index| {
                    let row = sigil::reactive::derived(move || {
                        card.with(|rows| rows.get(index).cloned())
                    });
                    container()
                        .layout(fill(LayoutStyle::block()))
                        .children_reactive(move || {
                            row.with(|row| {
                                row.iter()
                                    .map(|(x, y, cells)| placed(*x, *y, cells))
                                    .collect::<Vec<_>>()
                            })
                        })
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
