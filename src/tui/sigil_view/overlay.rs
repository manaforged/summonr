use sigil::prelude::{
    AlignItems, Geometry, IntoNode, KeyEvent, LayoutBuilderExt, LayoutStyle, ListRowSlots,
    LogicalKey, ModifiersState, NodeBuilder, NodeEventsExt, NodeStyleExt, Shortcut, Signal, Style,
    Tone, WidgetLayoutExt, classes, container, hint_bar, label, list_row_with, modal, notice_band,
    section_label,
};

use super::app::App;
use super::canvas;

pub(in crate::tui::sigil_view) type KeyAction = fn(&App, &mut KeyEvent);
pub(in crate::tui::sigil_view) type Bindings = Vec<(Shortcut, KeyAction)>;

const OPTION_GLYPHS: [(&str, &str); 3] = [("p", "π"), ("d", "∂"), ("o", "ø")];

pub(in crate::tui::sigil_view) fn press(
    key: &'static str,
    action: KeyAction,
) -> (Shortcut, KeyAction) {
    (LogicalKey::character(key).into(), action)
}

pub(in crate::tui::sigil_view) fn alt(letter: &'static str, action: KeyAction) -> Bindings {
    let mut bindings = vec![(
        Shortcut::new(LogicalKey::character(letter), ModifiersState::ALT),
        action,
    )];
    bindings.extend(
        OPTION_GLYPHS
            .iter()
            .filter(|(key, _)| *key == letter)
            .map(|(_, glyph)| press(glyph, action)),
    );
    bindings
}

pub(in crate::tui::sigil_view) fn keyed(
    node: NodeBuilder,
    app: &App,
    bindings: Bindings,
) -> NodeBuilder {
    bindings.into_iter().fold(node, |node, (shortcut, action)| {
        let app = app.clone();
        node.key_capture(shortcut, move |event| action(&app, event))
    })
}

pub(in crate::tui) fn shell(
    open: Signal<bool>,
    title: impl Into<String>,
    cols: u16,
    body: impl IntoNode + 'static,
) -> NodeBuilder {
    framed(
        open,
        title.into(),
        canvas::dialog_width(cols, canvas::DIALOG_CELLS),
        body,
    )
}

pub(in crate::tui) fn wide_shell(
    open: Signal<bool>,
    title: impl Into<String>,
    cols: u16,
    body: impl IntoNode + 'static,
) -> NodeBuilder {
    framed(
        open,
        title.into(),
        canvas::dialog_width(cols, canvas::WIDE_DIALOG_CELLS),
        body,
    )
}

fn framed(
    open: Signal<bool>,
    title: String,
    width: f32,
    body: impl IntoNode + 'static,
) -> NodeBuilder {
    container().layout(LayoutStyle::block()).child(
        modal(open)
            .title(title)
            .trap(true)
            .width(width)
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

pub(in crate::tui) fn heading(parts: Vec<(String, &'static str)>) -> NodeBuilder {
    parts.into_iter().fold(
        container().layout(row(8.0).align_items(AlignItems::Center)),
        |line, (text, tone)| line.child(label(text).class("text-xs").class(tone)),
    )
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
