use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, Signal, container, signal, slider,
};

use super::app::App;
use super::canvas;
use super::prelude::*;
use crate::provider::Effort;

#[derive(Clone, Copy)]
pub(super) struct Slider {
    pub(super) open: Signal<bool>,
    level: Signal<f32>,
}

#[expect(clippy::cast_precision_loss, reason = "four effort levels")]
fn position(level: usize) -> f32 {
    level as f32
}

impl Slider {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            level: signal(0.0),
        }
    }

    pub(super) fn show(self, current: Option<Effort>) {
        let current = current.unwrap_or(Effort::None);
        let at = Effort::ALL
            .iter()
            .position(|level| *level == current)
            .unwrap_or(0);
        self.level.set(position(at));
        self.open.set(true);
    }

    fn index(self) -> usize {
        (0..Effort::ALL.len())
            .min_by(|a, b| {
                (position(*a) - self.level.peek())
                    .abs()
                    .total_cmp(&(position(*b) - self.level.peek()).abs())
            })
            .unwrap_or(0)
    }

    pub(super) fn shift(self, delta: isize) {
        let last = Effort::ALL.len() - 1;
        self.level.set(position(
            self.index().saturating_add_signed(delta).min(last),
        ));
    }

    pub(super) fn picked(self) -> Effort {
        Effort::ALL
            .get(self.index())
            .copied()
            .unwrap_or(Effort::None)
    }
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let snapshot = app.model.peek().snapshot.clone();
    let current = snapshot.effort.unwrap_or(Effort::None);
    let reasons =
        crate::catalog::known(&snapshot.endpoint, &snapshot.model).map(|known| known.reasoning);
    let note = if reasons == Some(false) {
        format!("{} does not reason; effort has no effect", snapshot.model)
    } else {
        format!("reasoning effort for {}", snapshot.model)
    };
    let cols = app.viewport.peek().0;
    let width = canvas::dialog_width(cols, canvas::DIALOG_CELLS) - canvas::cell().w * 8.0;
    shell(
        app.effort.open,
        "Effort",
        cols,
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .child(group(note))
            .child(
                container()
                    .layout(LayoutStyle::block().fill_width().padding_x(16.0))
                    .child(
                        slider(app.effort.level, 0.0..=position(Effort::ALL.len() - 1))
                            .step(1.0)
                            .length(width)
                            .label("Effort")
                            .marks(Effort::ALL.map(Effort::as_str)),
                    ),
            )
            .child(hint(
                "←→ move · ↵ set · esc close",
                format!("now {}", current.as_str()),
            )),
    )
}
