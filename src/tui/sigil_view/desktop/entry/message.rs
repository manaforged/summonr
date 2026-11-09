use sigil::layout::Dimension;
use sigil::prelude::{
    AlignItems, JustifyContent, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder,
    NodeEventsExt, Paint, PointerEvent, Radius, Token, button, container, markdown, signal,
};

use super::super::App;
use crate::prefs::Visibility;
use crate::tui::sigil_view::chat::entry::label;
use crate::tui::sigil_view::chat::entry::reply::{self, Part, parts};
use crate::tui::sigil_view::text_style::TextStyle;

const FENCE: &str = "```";
const LINE: f32 = 22.0;

pub(super) fn user(body: String) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .min_size(0.0, 0.0)
                .justify_content(JustifyContent::End),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::block()
                        .padding_x(14.0)
                        .padding_y(10.0)
                        .max_width(Dimension::Percent(0.72))
                        .min_size(0.0, 0.0),
                )
                .bg(Token::SurfaceHover)
                .rounded(Radius::Lg)
                .child(TextStyle::sm(Token::Gray12).on(sigil::kernel::text(body))),
        )
}

fn code_block(app: &App, lang: String, body: String) -> NodeBuilder {
    let copy = app.clone();
    let copied = body.clone();
    let head = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(12.0)
                .padding_y(4.0)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::SpaceBetween),
        )
        .border_bottom(1)
        .border_color(Token::Border)
        .child(label(
            if lang.is_empty() {
                "text"
            } else {
                lang.as_str()
            },
            TextStyle::xs(Token::Gray10),
        ))
        .child(
            button("Copy")
                .ghost()
                .activate(move || copy.copy.set(Some(copied.clone())))
                .accessible("Copy code"),
        );
    container()
        .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
        .bg(Token::Field)
        .border_w(1)
        .border_color(Token::Border)
        .rounded(Radius::Md)
        .child(head)
        .child(markdown(signal(format!("{FENCE}{lang}\n{body}\n{FENCE}"))))
}

pub(super) fn assistant(body: &str, app: &App) -> NodeBuilder {
    let mut column = container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .gap(10.0)
            .min_size(0.0, 0.0),
    );
    if body.trim().is_empty() {
        return column;
    }
    let layout = app.model.get().prefs.diff_layout;
    for block in parts(body) {
        column = column.child(match block {
            Part::Prose(text) => container()
                .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
                .line_height_px(LINE)
                .child(markdown(signal(text))),
            Part::Code { lang, body, closed } => {
                if !closed && let Some(card) = reply::diff_card(app, &body) {
                    card
                } else {
                    code_block(app, lang, body)
                }
            }
            Part::Shown(shown) => reply::paint(shown, app, layout),
        });
    }
    column
}

pub(super) fn thinking(body: &str, visibility: Visibility, live: bool, app: &App) -> NodeBuilder {
    if body.trim().is_empty() {
        return container();
    }
    let toggle = app.clone();
    let open = visibility == Visibility::Expanded;
    let next = if open {
        Visibility::Collapsed
    } else {
        Visibility::Expanded
    };
    let words = summonr_core::text::plural(body.split_whitespace().count(), "word");
    let head = if live {
        "Thinking…".to_string()
    } else {
        format!("Thought · {words}")
    };
    let row = container()
        .layout(
            LayoutStyle::flex_row()
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .child(label(
            if open { "▾" } else { "▸" },
            TextStyle::sm(Token::Gray10),
        ))
        .child(label(head, TextStyle::sm(Token::Gray10)));
    let mut column = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(4.0))
        .pointer_up(move |event: &mut PointerEvent| {
            if event.button == Some(MouseButton::Left) {
                toggle.set_visibility(|prefs| &mut prefs.reasoning, Some(next));
                event.stop_propagation();
            }
        })
        .child(row);
    if open {
        column = column.child(
            TextStyle::sm(Token::Gray10)
                .italic()
                .on(sigil::kernel::text(body.to_string()))
                .line_height_px(LINE),
        );
    }
    column
}

pub(super) fn quiet(body: String) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
        .child(TextStyle::xs(Token::Gray10).on(sigil::kernel::text(body)))
}

pub(super) fn error(reason: String) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Start),
        )
        .child(label("⚠", TextStyle::xs(Token::Error11)))
        .child(TextStyle::xs(Token::Error11).on(sigil::kernel::text(reason)))
}

#[cfg(test)]
#[path = "message_tests.rs"]
mod tests;
