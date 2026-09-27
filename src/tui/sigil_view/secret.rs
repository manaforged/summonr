use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, classes, container, label,
};

use super::app::App;
use super::canvas;
use super::prelude::*;
use crate::text::ellipsize;

pub(super) fn layer(app: App) -> NodeBuilder {
    let secret = app.secret;
    let cols = app.viewport.peek().0;
    shell(
        app.secret_dialog,
        "Credential required",
        cols,
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .gap(canvas::overlay_gap()),
                    )
                    .children_reactive(move || {
                        let (prompt, typed) = secret.get().unwrap_or_default();
                        let prompt = if prompt.is_empty() {
                            "secret".to_string()
                        } else {
                            prompt
                        };
                        vec![
                            group(ellipsize(&prompt, canvas::DIALOG_CELLS as usize - 8)),
                            container()
                                .layout(LayoutStyle::block().padding_x(8.0))
                                .child(
                                    label(format!("{}▌", "•".repeat(typed.min(48))))
                                        .class(classes!("text-sm")),
                                ),
                        ]
                    }),
            )
            .child(hint(
                "↵ submit · esc cancel",
                "goes to the command, not the model",
            )),
    )
}
