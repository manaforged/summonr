use sigil::prelude::AlignItems;
use sigil::prelude::IntoNode;
use sigil::prelude::KeyEvent;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::ListRowSlots;
use sigil::prelude::LogicalKey;
use sigil::prelude::ModifiersState;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::Paint;
use sigil::prelude::Shortcut;
use sigil::prelude::Signal;
use sigil::prelude::Tone;
use sigil::prelude::WidgetLayoutExt;
use sigil::prelude::container;
use sigil::prelude::hint_bar;
use sigil::prelude::list_row_with;
use sigil::prelude::modal;
use sigil::prelude::notice_band;
use sigil::prelude::section_label;

use super::app::App;
use super::canvas;
use super::look::Look;

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

pub(in crate::tui) fn wide_page(
    open: Signal<bool>,
    title: impl Into<String>,
    cols: u16,
    body: impl IntoNode + 'static,
) -> NodeBuilder {
    framed(
        open,
        title.into(),
        canvas::dialog_width(cols, f32::from(cols)),
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
                    .rounded(sigil::prelude::Radius::Base)
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

pub(in crate::tui) fn heading(parts: Vec<(String, Look)>) -> NodeBuilder {
    parts.into_iter().fold(
        container().layout(row(8.0).align_items(AlignItems::Center)),
        |line, (text, look)| line.child(look.on(sigil::kernel::text(text).nowrap())),
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
        .bg(canvas::blend(canvas::palette().base, color, 0.14))
}
