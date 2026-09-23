use sigil::prelude::{
    AlignItems, ColorAtom, Geometry, IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder,
    NodeStyleExt, Signal, Style, Tone, WidgetLayoutExt, classes, container, hint_bar, label, modal,
    notice_band, section_label,
};

use super::canvas;

pub(in crate::tui) fn shell(
    open: Signal<bool>,
    title: impl Into<String>,
    body: impl IntoNode + 'static,
) -> NodeBuilder {
    container().layout(LayoutStyle::block()).child(
        modal(open)
            .title(title.into())
            .trap(true)
            .width(canvas::dialog_width(80, 72.0))
            .border_w(canvas::cell().h as i16)
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .padding_x(8.0)
                            .padding_y(8.0)
                            .gap(canvas::overlay_gap()),
                    )
                    .class(classes!("rounded"))
                    .child(body),
            ),
    )
}

fn accent() -> sigil::prelude::Rgba8 {
    canvas::palette().accent
}

fn tint() -> sigil::prelude::Rgba8 {
    canvas::blend(canvas::palette().base, canvas::palette().accent, 0.18)
}

pub(in crate::tui) fn pick(
    selected: bool,
    current: bool,
    name: impl Into<String>,
    context: impl Into<String>,
) -> NodeBuilder {
    let cell = canvas::cell();
    let name = name.into();
    let context = context.into();
    let cursor = if selected { "❯" } else { " " };
    let mut row = container()
        .layout(
            row(0.0)
                .padding_right(8.0)
                .gap(cell.w)
                .align_items(AlignItems::Center),
        )
        .child(
            container()
                .layout(LayoutStyle::block().width(cell.w * 2.0))
                .child(label(cursor).style(Style::new().text_color(ColorAtom::Rgba8(accent())))),
        )
        .child(
            label(if current { format!("● {name}") } else { name })
                .ellipsize()
                .class(classes!("text-sm"))
                .style(Style::new().text_color(ColorAtom::Rgba8(canvas::palette().text))),
        );
    row = row.child(container().layout(LayoutStyle::block().grow(1.0)));
    if !context.is_empty() {
        row = row.child(
            label(context)
                .ellipsize()
                .class(classes!("text-xs"))
                .style(Style::new().text_color(ColorAtom::Rgba8(canvas::palette().dim))),
        );
    }
    if selected {
        row = row.style(Style::new().bg(tint()));
    }
    row
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
    hint_bar(keys.into(), right.into()).layout(row(16.0).align_items(AlignItems::Center))
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
