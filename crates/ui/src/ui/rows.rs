use sigil::prelude::{
    AlignItems, ListRowSlots, NodeBuilder, Paint, Signal, Space, Token, Tone, container, list_row,
    notice_band, section_label, text_edit,
};
use sigil::widgets::{RowRender, TextEditBuilder, VirtualListBuilder};

use super::input_keys;
use crate::app::App;
use crate::canvas;
use crate::text_style::TextStyle;
use sigil::prelude::Widget;

pub(super) fn row<W: Widget>(node: W, padding: Space) -> W {
    node.fill_width()
        .height(canvas::overlay_list_row())
        .padding_x(padding)
}

pub(crate) fn pick(
    selected: bool,
    name: impl Into<String>,
    context: impl Into<String>,
) -> NodeBuilder {
    let context = context.into();
    let slots = ListRowSlots {
        status: (!context.is_empty()).then(|| context.into()),
        ..ListRowSlots::default()
    };
    slotted(selected, None, name, slots)
}

pub(crate) fn slotted(
    selected: bool,
    mark: Option<&str>,
    name: impl Into<String>,
    slots: ListRowSlots,
) -> NodeBuilder {
    let cursor = if canvas::is_window() {
        mark.unwrap_or(" ").to_string()
    } else {
        format!(
            "{}{}",
            if selected { "❯" } else { " " },
            mark.unwrap_or(" ")
        )
    };
    list_row(
        selected,
        name.into(),
        ListRowSlots {
            marker: Some(cursor.into()),
            ..slots
        },
    )
}

pub(crate) fn group(title: impl Into<String>) -> NodeBuilder {
    row(section_label(title.into()), Space::Sm)
}

pub(crate) fn heading(parts: Vec<(String, TextStyle)>) -> NodeBuilder {
    parts.into_iter().fold(
        row(sigil::prelude::row().gap(Space::None), Space::Sm).align_items(AlignItems::Center),
        |line, (text, text_style)| line.child(text_style.on(sigil::prelude::text(text).nowrap())),
    )
}

pub(crate) fn notice(text: impl Into<String>, tone: Tone) -> NodeBuilder {
    let color = if matches!(tone, Tone::Error) {
        canvas::palette().error
    } else {
        canvas::palette().dim
    };
    row(notice_band(text.into(), tone), Space::Sm).background(canvas::blend(
        canvas::palette().base,
        color,
        0.14,
    ))
}

const FIELD_KEY: usize = 11;

pub(crate) fn part(value: impl Into<String>, color: Token) -> (String, TextStyle) {
    (value.into(), TextStyle::sm(color))
}

pub(crate) fn field(key: &str, value: impl Into<String>, color: Token) -> NodeBuilder {
    heading(vec![
        part(format!("{key:<FIELD_KEY$}"), Token::Gray10),
        part(value, color),
    ])
}

pub(crate) fn blank() -> NodeBuilder {
    heading(vec![part("", Token::Gray10)])
}

pub(crate) const fn field_width(width: usize) -> usize {
    width.saturating_sub(FIELD_KEY)
}

pub(crate) fn overlay_list<T: 'static, R: RowRender<T>>(
    list: VirtualListBuilder<T, R>,
    row: f32,
    shown: usize,
) -> NodeBuilder {
    let height = row * shown as f32;
    container()
        .fill_width()
        .child(list.row_height(row).height(height).viewport_height(height))
}

pub(crate) fn browser_split(cols: u16, threshold: u16) -> bool {
    cols >= threshold && !canvas::is_window()
}

pub(crate) fn browser_layout(
    (cols, rows): (u16, u16),
    threshold: u16,
    (chrome, below): (u16, u16),
) -> (bool, usize, usize) {
    let split = browser_split(cols, threshold);
    (
        split,
        super::Size::Page.cells((cols, rows)),
        canvas::list_rows(
            (cols, rows),
            chrome.saturating_add(if split { 0 } else { below }),
        ),
    )
}

pub(crate) fn browser_columns(
    list: impl IntoIterator<Item = NodeBuilder>,
    frame: NodeBuilder,
    detail: Vec<NodeBuilder>,
    gap: Space,
) -> Vec<NodeBuilder> {
    let list = list
        .into_iter()
        .fold(frame, |node, child| node.child(child));
    let detail = detail.into_iter().fold(
        sigil::prelude::column()
            .fill_width()
            .gap(canvas::overlay_gap()),
        |node, child| node.child(child),
    );
    vec![
        sigil::prelude::row()
            .align_items(sigil::prelude::AlignItems::Stretch)
            .fill_width()
            .gap(gap)
            .child(list)
            .child(detail),
    ]
}

pub(crate) fn filter_field(
    app: &App,
    query: Signal<String>,
    caret: Signal<usize>,
) -> TextEditBuilder {
    input_keys(app, text_edit(query))
        .caret_position(caret)
        .placeholder(super::FILTER_HINT)
        .height(canvas::overlay_list_row())
}

pub(crate) fn filter_column() -> NodeBuilder {
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .grow(1.0)
        .min_width(0.0)
        .min_height(0.0)
}

pub(crate) fn plain(text: impl Into<String>) -> NodeBuilder {
    container().child(
        sigil::prelude::text(text.into())
            .nowrap()
            .text_color(Token::OnSurface),
    )
}
