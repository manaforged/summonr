use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

pub(super) use super::app::App;
pub(super) use super::chat;

use super::canvas::write_at;
use super::canvas::{Cell, Ink, blend, cell as metrics, ink, palette, rgb, shown};
use super::model::Model;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Position, Rgba8, Style, container,
    label,
};

pub(in crate::tui::sigil_view) use input::prompt_area;

mod logo;
pub(in crate::tui::sigil_view) use logo::{compose, scene_row};
pub(in crate::tui::sigil_view) use space::space;

mod input;
mod space;

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset(0.0)
                .fill_width()
                .fill_height()
                .gap(0.0),
        )
        .children_reactive(move || {
            let (cols, rows) = app.viewport.get();
            drop(app.theme_name.get());
            let ticks = app.ticks.get();
            let ticks = if app.model.get().prefs.animations {
                ticks
            } else {
                0
            };
            let mut cells = space(cols, rows, ticks);
            compose(&mut cells, cols, rows, &app, ticks);
            cells
                .chunks(usize::from(cols).max(1))
                .map(scene_row)
                .collect::<Vec<_>>()
        })
}
