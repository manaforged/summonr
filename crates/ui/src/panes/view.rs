use sigil::prelude::Length;
use sigil::prelude::ObjectFit;
use sigil::prelude::Overflow;
use sigil::prelude::Space;
use sigil::prelude::{AlignItems, NodeBuilder, Paint, button, container, image, node, text_edit};

use super::body::{close, dim};
use super::{App, Pane};
use crate::text_style::TextStyle;
use crate::ui::input_keys;
use sigil::prelude::{Token, Widget};

pub(super) fn view(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let Some(view) = model.view.as_ref() else {
        return column.child(dim("no file open".to_string()));
    };
    let editing = app.view_edit.get();
    let edit = app.clone();
    let save = app.clone();
    let leave = app.clone();
    let mut bar = sigil::prelude::row()
        .fill_width()
        .gap(Space::Sm)
        .align_items(AlignItems::Center)
        .child(
            container()
                .grow(1.0)
                .min_width(0.0)
                .min_height(14.0)
                .overflow_x(Overflow::Hidden)
                .child(
                    sigil::prelude::text(view.path.clone())
                        .nowrap()
                        .font_size(14.0, "14 px text that keeps its own line height and weight")
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
            input_keys(&app, app.view_text, text_edit(app.view_text))
                .caret_position(app.view_caret)
                .autofocus()
                .gap(sigil::prelude::Space::None)
                .fill_width()
                .height(Length::Auto)
                .grow(1.0)
                .min_width(0.0)
                .min_height(0.0)
                .font_family(sigil::prelude::FontFamily::Mono)
                .font_size(
                    12.0,
                    "12 px chrome text that keeps its own line height and weight",
                )
                .background(sigil::prelude::Token::Gray1)
        }));
    }
    let text = &view.text;
    let added = |number: usize| {
        view.added
            .iter()
            .any(|(start, count)| number >= *start && number < start + count)
    };
    let mut body = sigil::prelude::column()
        .fill_width()
        .gap(Space::raw(1.0, "a one pixel row separation"))
        .radius(sigil::prelude::Radius::Md)
        .background(sigil::prelude::Token::Gray1);
    let scroll = app.pane_list(Pane::View).cursor.get();
    for (index, line) in text.lines().enumerate().skip(scroll) {
        let number = index + 1;
        let mut row = sigil::prelude::row()
            .align_items(sigil::prelude::AlignItems::Stretch)
            .fill_width()
            .padding_x(Space::Sm)
            .padding_y(Space::raw(1.0, "one pixel above and below a row"))
            .gap(Space::raw(10.0, "between the small and medium steps"));
        if added(number) {
            row = row.background(sigil::prelude::Token::Success3);
        }
        body = body.child(
            row.child(TextStyle::xs(Token::Gray11).line(number.to_string()))
                .child(
                    TextStyle::xs(Token::Gray12)
                        .mono()
                        .line(line.replace('\t', "    "))
                        .ellipsize()
                        .grow(1.0)
                        .min_width(0.0)
                        .min_height(14.0),
                ),
        );
    }
    column = column.child(body);
    if let Some(source) = view.image.clone() {
        let height = (source.height() as f32).min(320.0);
        column = column.child(
            image(source)
                .fit(ObjectFit::Contain)
                .fill_width()
                .min_width(0.0)
                .min_height(height),
        );
    }
    column
}
