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
pub(in crate::tui::sigil_view) use logo::{compose, placed, scene_row};
pub(in crate::tui::sigil_view) use space::{QUIET, SCRIM, backdrop, moving, step_sky};

mod galaxy;
mod input;
mod space;

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let sky = app.clone();
    let stars = app.clone();
    container()
        .bg(SCRIM)
        .layout(fill(LayoutStyle::flex_column()))
        .child(
            container()
                .layout(fill(LayoutStyle::flex_column()))
                .children_reactive(move || {
                    let (cols, rows) = sky.viewport.get();
                    drop(sky.theme_name.get());
                    backdrop(cols, rows, sky.sky.get())
                        .chunks(usize::from(cols).max(1))
                        .map(scene_row)
                        .collect::<Vec<_>>()
                }),
        )
        .child(
            container()
                .layout(fill(LayoutStyle::block()))
                .children_reactive(move || {
                    let (cols, rows) = stars.viewport.get();
                    drop(stars.theme_name.get());
                    let ticks = if stars.model.with(|model| model.prefs.animations) {
                        stars.ticks.get()
                    } else {
                        0
                    };
                    moving(cols, rows, ticks, stars.sky.get())
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
                    if !app.splash_revealed.get() {
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
