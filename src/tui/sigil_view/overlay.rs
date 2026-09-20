use sigil::prelude::{
    AlignItems, ColorAtom, IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt,
    Signal, Style, WidgetLayoutExt, classes, container, label, modal,
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
    let height = canvas::overlay_list_row();
    let name = name.into();
    let context = context.into();
    let cursor = if selected { "❯" } else { " " };
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(height)
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

pub(in crate::tui) fn group(title: impl Into<String>) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::overlay_list_row())
                .padding_x(8.0),
        )
        .child(
            label(title.into())
                .class(classes!("text-xs"))
                .style(Style::new().text_color(ColorAtom::Rgba8(canvas::palette().dim))),
        )
}

pub(in crate::tui) fn hint(keys: impl Into<String>, right: impl Into<String>) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::overlay_list_row())
                .padding_x(16.0)
                .align_items(AlignItems::Center),
        )
        .child(
            label(keys.into())
                .class(classes!("text-xs"))
                .style(Style::new().text_color(ColorAtom::Rgba8(canvas::palette().dim))),
        )
        .child(container().layout(LayoutStyle::block().grow(1.0)))
        .child(
            label(right.into())
                .class(classes!("text-xs"))
                .style(Style::new().text_color(ColorAtom::Rgba8(accent()))),
        )
}

pub(in crate::tui) fn band(text: impl Into<String>, error: bool) -> NodeBuilder {
    let tone = if error {
        canvas::palette().error
    } else {
        canvas::palette().dim
    };
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::overlay_list_row())
                .padding_x(8.0),
        )
        .style(Style::new().bg(canvas::blend(canvas::palette().base, tone, 0.14)))
        .child(
            label(text.into())
                .ellipsize()
                .class(classes!("text-xs"))
                .style(Style::new().text_color(ColorAtom::Rgba8(tone))),
        )
}
