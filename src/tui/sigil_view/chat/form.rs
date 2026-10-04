use sigil::prelude::Paint;
use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Tone, button, container, notice_band,
};

use super::App;
use crate::tui::sigil_view::app::Reply;

pub(in crate::tui::sigil_view) fn form_band(
    app: App,
    width: usize,
    lines: Vec<String>,
    review: bool,
) -> NodeBuilder {
    let mut lines = lines.into_iter();
    let head = lines.next().unwrap_or_default();
    let mut band = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(4.0))
        .child(notice_band(
            crate::text::ellipsize(&head, width.saturating_sub(4)),
            Tone::Info,
        ));
    for line in lines {
        band = band.child(
            sigil::kernel::text(crate::text::ellipsize(&line, width.saturating_sub(4)))
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface),
        );
    }
    band.child(actions(app, review))
}

fn actions(app: App, review: bool) -> NodeBuilder {
    let mut row = container().layout(
        LayoutStyle::flex_row()
            .gap(12.0)
            .align_items(AlignItems::Center),
    );
    if review {
        let send = app.clone();
        let decline = app.clone();
        row = row
            .child(
                button("Send")
                    .primary()
                    .activate(move || send.form_reply(Reply::Send)),
            )
            .child(button("Decline").activate(move || decline.form_reply(Reply::Decline)));
    }
    row.child(
        button("Cancel")
            .destructive()
            .activate(move || app.form_reply(Reply::Cancel)),
    )
}
