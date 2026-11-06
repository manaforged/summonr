use sigil::prelude::{AlignItems, LayoutStyle, NodeBuilder, Widget, container};

use super::super::style::{GUTTER, M, MARK, S, Text, text};
use crate::app::App;

const LIFT: f32 = 5.0;

/// The working row above the composer: a spinner and how long the agent has run.
pub(super) fn row(app: &App) -> NodeBuilder {
    let clock = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_left(GUTTER)
                .padding_right(GUTTER)
                .padding_bottom(S)
                .gap(M)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::block()
                        .width(MARK)
                        .height(MARK + LIFT)
                        .shrink(0.0),
                )
                .child(
                    sigil::widgets::spinner().layout(LayoutStyle::block().width(MARK).height(MARK)),
                ),
        )
        .child(container().layout(LayoutStyle::block().grow(1.0)))
        .child(
            text(Text::Meta, move || {
                clock.ticks.with(|_| ());
                crate::desktop::host::summary(&clock)
                    .and_then(|(_, since)| since)
                    .map(|since| summonr_core::text::duration(since.elapsed().as_secs()))
                    .unwrap_or_default()
            })
            .nowrap(),
        )
}
