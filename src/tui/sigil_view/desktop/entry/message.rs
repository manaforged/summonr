use sigil::layout::Dimension;
use sigil::prelude::{
    AlignItems, JustifyContent, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder,
    NodeEventsExt, Paint, PointerEvent, Radius, Token, button, container, markdown, signal,
};

use super::super::App;
use crate::prefs::Visibility;
use crate::tui::sigil_view::chat::entry::label;
use crate::tui::sigil_view::look::Look;

const FENCE: &str = "```";
const LINE: f32 = 22.0;

enum Block {
    Prose(String),
    Code { lang: String, body: String },
}

fn blocks(body: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut prose = Vec::new();
    let mut code: Option<(String, Vec<&str>)> = None;
    for line in body.lines() {
        let fence = line.trim_start().strip_prefix(FENCE);
        match (code.take(), fence) {
            (Some((lang, lines)), Some(_)) => out.push(Block::Code {
                lang,
                body: lines.join("\n"),
            }),
            (Some((lang, mut lines)), None) => {
                lines.push(line);
                code = Some((lang, lines));
            }
            (None, Some(lang)) => {
                if !prose.is_empty() {
                    out.push(Block::Prose(prose.join("\n")));
                    prose.clear();
                }
                code = Some((lang.trim().to_string(), Vec::new()));
            }
            (None, None) => prose.push(line),
        }
    }
    if let Some((lang, lines)) = code {
        out.push(Block::Code {
            lang,
            body: lines.join("\n"),
        });
    }
    if !prose.is_empty() {
        out.push(Block::Prose(prose.join("\n")));
    }
    out
}

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
                .child(Look::sm(Token::Gray12).on(sigil::kernel::text(body))),
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
            Look::xs(Token::Gray10),
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
    for block in blocks(body) {
        column = column.child(match block {
            Block::Prose(text) => container()
                .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
                .line_height_px(LINE)
                .child(markdown(signal(text))),
            Block::Code { lang, body } => code_block(app, lang, body),
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
    let next = if open { "collapsed" } else { "expanded" };
    let words = crate::text::plural(body.split_whitespace().count(), "word");
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
        .child(label(if open { "▾" } else { "▸" }, Look::sm(Token::Gray10)))
        .child(label(head, Look::sm(Token::Gray10)));
    let mut column = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(4.0))
        .pointer_up(move |event: &mut PointerEvent| {
            if event.button == Some(MouseButton::Left) {
                toggle.reasoning_command(Some(next));
                event.stop_propagation();
            }
        })
        .child(row);
    if open {
        column = column.child(
            Look::sm(Token::Gray10)
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
        .child(Look::xs(Token::Gray10).on(sigil::kernel::text(body)))
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
        .child(label("⚠", Look::xs(Token::Error11)))
        .child(Look::xs(Token::Error11).on(sigil::kernel::text(reason)))
}

#[cfg(test)]
mod tests {
    use super::{Block, blocks};

    #[test]
    fn fenced_code_splits_out_with_its_language() {
        let parts = blocks("intro\n```rust\nfn a() {}\n```\nafter");
        let shapes: Vec<(bool, String)> = parts
            .into_iter()
            .map(|block| match block {
                Block::Prose(text) => (false, text),
                Block::Code { lang, body } => (true, format!("{lang}:{body}")),
            })
            .collect();
        assert_eq!(
            shapes,
            vec![
                (false, "intro".to_string()),
                (true, "rust:fn a() {}".to_string()),
                (false, "after".to_string()),
            ]
        );
    }
}
