use sigil::prelude::Paint;
use sigil::prelude::Space;
use sigil::prelude::{NodeBuilder, container};

use super::app::App;
use super::canvas;
use super::ui::{Dialog, Hints, Key, Size, group};
use crate::cells::ellipsize;
use sigil::prelude::Widget;

pub(super) fn layer(app: App) -> NodeBuilder {
    let secret = app.secret;
    let viewport = app.viewport.peek();
    let hints = Hints::new()
        .key(Key::Enter, "submit")
        .key(Key::Esc, "cancel")
        .right("goes to the command, not the model");
    Dialog::new(Size::Normal, "Credential required", hints)
        .child(
            sigil::prelude::column()
                .fill_width()
                .gap(canvas::overlay_gap())
                .children_reactive(move || {
                    let (prompt, typed) = secret.get().unwrap_or_default();
                    let prompt = if prompt.is_empty() {
                        "secret".to_string()
                    } else {
                        prompt
                    };
                    vec![
                        group(ellipsize(&prompt, canvas::DIALOG_CELLS as usize - 8)),
                        container().padding_x(Space::Sm).child(
                            sigil::prelude::text(format!("{}▌", "•".repeat(typed.min(48))))
                                .nowrap()
                                .text_color(sigil::prelude::Token::OnSurface)
                                .font_size(
                                    14.0,
                                    "14 px text that keeps its own line height and weight",
                                ),
                        ),
                    ]
                }),
        )
        .build(app.secret_dialog, viewport, &app.model.peek().prefs.hotkeys)
}
