use sigil::kernel::ObjectFit;
use sigil::layout::Dimension;
use sigil::layout::Overflow;
use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, WidgetLayoutExt, button,
    container, image, node, text_edit,
};

use super::body::{close, dim};
use super::{App, Pane};

pub(super) fn view(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let Some(view) = model.view.as_ref() else {
        return column.child(dim("no file open".to_string()));
    };
    let editing = app.view_edit.peek();
    let edit = app.clone();
    let save = app.clone();
    let leave = app.clone();
    let mut bar = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::block()
                        .grow(1.0)
                        .min_size(0.0, 14.0)
                        .overflow_x(Overflow::Hidden),
                )
                .child(
                    sigil::kernel::text(view.path.clone())
                        .nowrap()
                        .font_size(14.0)
                        .font_weight(500)
                        .text_color(sigil::prelude::Token::Gray12),
                ),
        );
    if editing {
        bar = bar
            .child(button("Save").primary().activate(move || save.edit_save()))
            .child(button("Leave").ghost().activate(move || {
                leave.edit_stop();
            }));
    } else {
        bar = bar.child(button("Edit").ghost().activate(move || edit.edit_start()));
    }
    bar = bar.child(close(app.clone()));
    column = column.child(bar);
    if editing {
        return column.child(node(move || {
            text_edit(app.view_text)
                .caret_position(app.view_caret)
                .autofocus()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .height(Dimension::Auto)
                        .grow(1.0)
                        .min_size(0.0, 0.0),
                )
                .font_family(sigil::prelude::FontFamily::Mono)
                .font_size(12.0)
                .bg(sigil::prelude::Token::Gray1)
        }));
    }
    let text = &view.text;
    let added = |number: usize| {
        view.added
            .iter()
            .any(|(start, count)| number >= *start && number < start + count)
    };
    let mut body = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(1.0))
        .rounded(sigil::prelude::Radius::Md)
        .bg(sigil::prelude::Token::Gray1);
    let scroll = app.pane_list(Pane::View).cursor.get();
    for (index, line) in text.lines().enumerate().skip(scroll) {
        let number = index + 1;
        let mut row = container().layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(8.0)
                .padding_y(1.0)
                .gap(10.0),
        );
        if added(number) {
            row = row.bg(sigil::prelude::Token::Success3);
        }
        body = body.child(
            row.child(
                sigil::kernel::text(number.to_string())
                    .nowrap()
                    .font_size(12.0)
                    .text_color(sigil::prelude::Token::Gray11),
            )
            .child(
                sigil::kernel::text(line.replace('\t', "    "))
                    .nowrap()
                    .ellipsize()
                    .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                    .font_size(12.0)
                    .font_family(sigil::prelude::FontFamily::Mono)
                    .text_color(sigil::prelude::Token::Gray12),
            ),
        );
    }
    column = column.child(body);
    if let Some(source) = view.image.clone() {
        let height = (source.height() as f32).min(320.0);
        column = column.child(
            image(source)
                .fit(ObjectFit::Contain)
                .layout(LayoutStyle::block().fill_width().min_size(0.0, height)),
        );
    }
    column
}
