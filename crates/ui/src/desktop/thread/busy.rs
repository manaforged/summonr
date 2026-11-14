use sigil::prelude::{AlignItems, NodeBuilder, Widget, container};

use super::super::style::{L, M, MARK, S, Text, text};
use crate::app::App;

const LIFT: f32 = 5.0;

/// The working row above the composer: a spinner, what the agent is doing, and how long it has run.
pub(super) fn row(app: &App) -> NodeBuilder {
    let (clock, words, count) = (app.clone(), app.clone(), app.clone());
    sigil::prelude::row()
        .fill_width()
        .padding_left(L)
        .padding_right(L)
        .padding_bottom(S)
        .gap(M)
        .min_width(0.0)
        .min_height(0.0)
        .align_items(AlignItems::Center)
        .child(
            container()
                .width(MARK)
                .height(MARK + LIFT)
                .shrink(0.0)
                .child(sigil::widgets::spinner().width(MARK).height(MARK)),
        )
        .child(
            container().grow(1.0).min_width(0.0).min_height(0.0).child(
                text(Text::Meta, move || {
                    crate::desktop::host::summary(&words)
                        .map(|(words, _)| words)
                        .unwrap_or_default()
                })
                .ellipsize(),
            ),
        )
        .child(
            text(Text::Meta, move || {
                count.model.with(|model| {
                    let tokens = model.tokens(|round| round.prompt)
                        + model.tokens(|round| round.completion)
                        + model.speed.streamed();
                    if tokens == 0 {
                        return String::new();
                    }
                    format!("{} tokens", summonr_agent::provider::compact(tokens))
                })
            })
            .nowrap(),
        )
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
