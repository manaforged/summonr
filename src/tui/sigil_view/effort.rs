use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, Paint,
    Signal, Token, container, signal,
};

use super::app::App;
use super::canvas;
use super::look::Look;
use crate::provider::Effort;

#[derive(Clone, Copy)]
pub(super) struct Strip {
    pub(super) open: Signal<bool>,
    cursor: Signal<Effort>,
}

impl Strip {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            cursor: signal(Effort::None),
        }
    }

    pub(super) fn show(self, current: Option<Effort>, levels: &[Effort]) {
        self.cursor.set(
            current
                .map(|level| level.clamp(levels))
                .unwrap_or(levels.first().copied().unwrap_or(Effort::None)),
        );
        self.open.set(true);
    }

    pub(super) fn shift(self, levels: &[Effort], delta: isize) {
        if levels.is_empty() {
            return;
        }
        let at = self.cursor.peek().clamp(levels);
        self.cursor.set(at.shifted(levels, delta));
    }

    pub(super) fn pick(self, level: Effort) {
        self.cursor.set(level);
    }

    pub(super) fn picked(self) -> Effort {
        self.cursor.peek()
    }
}

pub(super) fn strip(app: App) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(LayoutStyle::flex_column().fill_width().shrink(0.0))
        .children_reactive(move || {
            if !update.effort.open.get() {
                return Vec::new();
            }
            let model = update.model.get();
            let endpoint = model.snapshot.endpoint.clone();
            let reasons = crate::catalog::known(&endpoint, &model.snapshot.model)
                .map(|known| known.reasoning);
            vec![card(
                update.clone(),
                model.snapshot.model.clone(),
                model.snapshot.effort,
                model.snapshot.levels.clone(),
                reasons,
            )]
        })
}

fn card(
    app: App,
    model: String,
    current: Option<Effort>,
    levels: Vec<Effort>,
    reasons: Option<bool>,
) -> NodeBuilder {
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .align_items(AlignItems::Center)
                .padding_x(8.0)
                .gap(8.0),
        )
        .child(label("effort", Token::Gray10, false));
    if levels.is_empty() {
        row = row.child(label(
            &format!("{model} takes no effort level"),
            Token::Gray11,
            false,
        ));
    } else {
        if reasons == Some(false) {
            row = row.child(label("no effect", Token::Gray11, false));
        }
        let cursor = app.effort.picked().clamp(&levels);
        for level in levels {
            let app = app.clone();
            let chosen = level == cursor;
            row = row.child(
                container()
                    .layout(
                        LayoutStyle::flex_row()
                            .shrink(0.0)
                            .align_items(AlignItems::Center),
                    )
                    .child(label(
                        level.as_str(),
                        if chosen {
                            Token::Accent11
                        } else {
                            Token::Gray10
                        },
                        chosen,
                    ))
                    .pointer_down(move |event| {
                        if event.button == Some(MouseButton::Left) {
                            app.effort.pick(level);
                            app.commit_effort();
                            event.stop_propagation();
                        }
                    }),
            );
        }
        row = row.child(label(
            &format!("now {}", current.map_or("off", Effort::as_str)),
            Token::Gray9,
            false,
        ));
    }
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_x(8.0)
                .padding_bottom(canvas::overlay_gap()),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_row().fill_width())
                .border_w(canvas::frame_border())
                .border_color(Token::Gray6)
                .bg(canvas::palette().tool_bg)
                .child(row),
        )
}

fn label(text: &str, color: Token, bold: bool) -> NodeBuilder {
    let look = Look::sm(color);
    let look = if bold { look.bold() } else { look };
    container().child(look.on(sigil::kernel::text(text).nowrap()))
}
