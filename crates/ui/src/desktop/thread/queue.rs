use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, NodeBuilder, Paint, Widget, button, container, spacer,
};

use super::super::style::{DIVIDER, S, Text, XS, text};
use crate::app::App;
use summonr_agent::hub::Activity;

const SHOWN: usize = 3;

/// Messages typed during a run, waiting to be sent, with Edit and Send now.
pub(super) fn panel(app: &App) -> Option<NodeBuilder> {
    let model = app.model.get();
    if model.pending.is_empty() {
        return None;
    }
    let waiting = model.pending.iter().filter(|(next, _)| !next).count();
    let steerable = waiting > 0
        && matches!(model.busy_since, Some((Activity::Turn, _)))
        && app.approval.get().is_none()
        && app.form.get().is_none();
    let (edit, send) = (app.clone(), app.clone());
    let mut head = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(XS)
                .align_items(AlignItems::Center),
        )
        .child(text(Text::Heading, heading(model.pending.len())))
        .child(spacer())
        .child(
            button("Edit")
                .ghost()
                .size(ControlSize::Compact)
                .activate(move || edit.dequeue()),
        );
    if steerable {
        head = head.child(
            button("Send now")
                .neutral()
                .soft()
                .size(ControlSize::Compact)
                .activate(move || send.steer_queued()),
        );
    }
    let mut list = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_left(S)
                .padding_right(XS)
                .padding_bottom(S)
                .gap(XS),
        )
        .border_bottom(1)
        .border_color(DIVIDER)
        .child(head);
    for (next, message) in model.pending.iter().take(SHOWN) {
        let first = message.lines().next().unwrap_or_default().to_string();
        list = list.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .gap(S)
                        .min_size(0.0, 0.0),
                )
                .child(text(Text::Meta, if *next { "Next" } else { "After this turn" }).nowrap())
                .child(text(Text::Label, first).ellipsize()),
        );
    }
    if model.pending.len() > SHOWN {
        list = list.child(text(
            Text::Meta,
            format!("+{} more", model.pending.len() - SHOWN),
        ));
    }
    Some(
        container()
            .layout(LayoutStyle::block().fill_width().padding_bottom(S))
            .child(list),
    )
}

fn heading(count: usize) -> String {
    match count {
        1 => "1 message queued".to_string(),
        count => format!("{count} messages queued"),
    }
}
