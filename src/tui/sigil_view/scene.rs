use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

pub(super) use super::app::App;

use super::canvas::write_at;
use super::canvas::{Cell, Ink, blend, cell as metrics, ink, palette, rgb, shown};
use super::model::Model;
use sigil::layout::Position;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Paint;
use sigil::prelude::Rgba8;
use sigil::prelude::container;

pub(in crate::tui::sigil_view) use input::prompt_area;

mod logo;
mod mark;
pub(in crate::tui::sigil_view) use logo::{compose, placed};
pub(in crate::tui::sigil_view) use mark::Mark;
pub(in crate::tui::sigil_view) use space::{PLATE, SCRIM, VOID, moving};

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
    let version = sigil::reactive::derived(move || {
        let (cols, rows) = shape.viewport.get();
        (u64::from(cols) << 16) | u64::from(rows)
    });
    let beats_per_second = 1.0 / crate::limits::limits().animation().as_secs_f64().max(0.001);
    container()
        .bg(VOID)
        .layout(fill(LayoutStyle::flex_column()))
        .child(
            sigil::widgets::canvas(move |scene, area| {
                galaxy::paint_sky(scene, area, painter.viewport.peek(), painter.sky);
            })
            .redraw_on(version)
            .layout(fill(LayoutStyle::block())),
        )
        .child(
            container()
                .layout(fill(LayoutStyle::block()))
                .children_reactive(move || {
                    let (cols, rows) = stars.viewport.get();
                    drop(stars.theme_name.get());
                    let beats = stars
                        .sky_clock
                        .get()
                        .map_or(0.0, |clock| clock.get() * beats_per_second);
                    moving(cols, rows, beats, stars.sky)
                        .into_iter()
                        .map(|spot| placed(spot.x, spot.y, &[spot.cell]))
                        .collect::<Vec<_>>()
                }),
        )
        .child(
            container()
                .layout(fill(LayoutStyle::block()))
                .children_reactive(move || {
                    let (cols, rows) = app.viewport.get();
                    drop(app.theme_name.get());
                    let _backdrop = app.backdrop.get();
                    let _draft = app.draft.get();
                    let moving = app.splash.get() && app.model.with(|model| model.prefs.animations);
                    if !app.splash_revealed.get() || moving {
                        let _reveal = app.ticks.get();
                    }
                    let mut cells = vec![Cell::blank(); usize::from(cols) * usize::from(rows)];
                    let [x, y, width, height] = compose(&mut cells, cols, rows, &app);
                    (y..y + height)
                        .map(|row| {
                            let start = usize::from(row) * usize::from(cols) + usize::from(x);
                            placed(x, row, &cells[start..start + usize::from(width)])
                        })
                        .collect::<Vec<_>>()
                }),
        )
}

fn fill(style: LayoutStyle) -> LayoutStyle {
    style
        .position(Position::Absolute)
        .inset(0.0)
        .fill_width()
        .fill_height()
        .gap(0.0)
}
