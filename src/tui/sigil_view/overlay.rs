use sigil::prelude::{
    AlignItems, Geometry, IntoNode, LayoutBuilderExt, LayoutStyle, ListRowSlots, NodeBuilder,
    NodeStyleExt, Signal, Style, Tone, WidgetLayoutExt, classes, container, hint_bar,
    list_row_with, modal, notice_band, section_label,
};

use super::canvas;

pub(in crate::tui) fn shell(
    open: Signal<bool>,
    title: impl Into<String>,
    cols: u16,
    body: impl IntoNode + 'static,
) -> NodeBuilder {
    container().layout(LayoutStyle::block()).child(
        modal(open)
            .title(title.into())
            .trap(true)
            .width(canvas::dialog_width(cols, 72.0))
            .border_w(canvas::cell().h as i16)
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .padding_x(8.0)
                            .padding_y(canvas::overlay_gap() * 4.0)
                            .gap(canvas::overlay_gap()),
                    )
                    .class(classes!("rounded"))
                    .child(body),
            ),
    )
}

pub(in crate::tui) fn pick(
    selected: bool,
    name: impl Into<String>,
    context: impl Into<String>,
) -> NodeBuilder {
    let context = context.into();
    let slots = ListRowSlots {
        status: (!context.is_empty()).then(|| context.into()),
        shrink: true,
        ..ListRowSlots::default()
    };
    slotted(selected, None, name, slots)
}

pub(in crate::tui) fn slotted(
    selected: bool,
    mark: Option<&str>,
    name: impl Into<String>,
    slots: ListRowSlots,
) -> NodeBuilder {
    let cursor = format!(
        "{}{}",
        if selected { "❯" } else { " " },
        mark.unwrap_or(" ")
    );
    list_row_with(
        selected,
        name.into(),
        ListRowSlots {
            marker: Some(cursor.into()),
            ..slots
        },
    )
}

fn row(padding: f32) -> LayoutStyle {
    LayoutStyle::flex_row()
        .fill_width()
        .height(canvas::overlay_list_row())
        .padding_x(padding)
}

pub(in crate::tui) fn group(title: impl Into<String>) -> NodeBuilder {
    section_label(title.into()).layout(row(8.0))
}

pub(in crate::tui) fn hint(keys: impl Into<String>, right: impl Into<String>) -> NodeBuilder {
    hint_bar(keys.into(), right.into()).layout(
        row(16.0)
            .align_items(AlignItems::Center)
            .gap(canvas::cell().w * 2.0),
    )
}

pub(in crate::tui) fn band(text: impl Into<String>, error: bool) -> NodeBuilder {
    let (tone, color) = if error {
        (Tone::Error, canvas::palette().error)
    } else {
        (Tone::Neutral, canvas::palette().dim)
    };
    notice_band(text.into(), tone)
        .layout(row(8.0))
        .style(Style::new().bg(canvas::blend(canvas::palette().base, color, 0.14)))
}
