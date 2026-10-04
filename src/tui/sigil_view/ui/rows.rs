use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, ListRowSlots, NodeBuilder, Paint, Tone, container,
    list_row_with, notice_band, section_label,
};

use crate::tui::sigil_view::canvas;
use crate::tui::sigil_view::look::Look;

pub(super) fn row(padding: f32) -> LayoutStyle {
    LayoutStyle::flex_row()
        .fill_width()
        .height(canvas::overlay_list_row())
        .padding_x(padding)
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

pub(in crate::tui) fn group(title: impl Into<String>) -> NodeBuilder {
    section_label(title.into()).layout(row(8.0))
}

pub(in crate::tui) fn heading(parts: Vec<(String, Look)>) -> NodeBuilder {
    parts.into_iter().fold(
        container().layout(row(8.0).align_items(AlignItems::Center)),
        |line, (text, look)| line.child(look.on(sigil::kernel::text(text).nowrap())),
    )
}

pub(in crate::tui) fn notice(text: impl Into<String>, tone: Tone) -> NodeBuilder {
    let color = if matches!(tone, Tone::Error) {
        canvas::palette().error
    } else {
        canvas::palette().dim
    };
    notice_band(text.into(), tone)
        .layout(row(8.0))
        .bg(canvas::blend(canvas::palette().base, color, 0.14))
}
