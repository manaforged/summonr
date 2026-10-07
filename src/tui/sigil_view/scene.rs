use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

pub(super) use super::app::App;
pub(super) use super::chat;

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
pub(in crate::tui::sigil_view) use logo::{compose, scene_row};
pub(in crate::tui::sigil_view) use space::{SCRIM, space};

mod input;
mod space;

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    container()
        .bg(SCRIM)
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
            let _backdrop = app.backdrop.get();
            let _draft = app.draft.get();
            let _caret = app.caret.get();
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
