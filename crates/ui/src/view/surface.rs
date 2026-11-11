use super::*;
use crate::ui::input_keys;
use sigil::prelude::Length;
use sigil::prelude::Paint;
use sigil::prelude::Space;
use sigil::prelude::Widget;
use sigil::reactive::untrack;
use sigil::widgets::TextEditBuilder;

pub(super) fn layer(app: App) -> NodeBuilder {
    let (cols, _rows) = app.viewport.get();
    let layout = app.layout.get();
    let (chat_cols, side_cols) = if app.inline {
        (cols, 0)
    } else {
        layout.split(cols)
    };
    let surface = chat_surface(app.clone(), chat_cols);
    let row = sigil::prelude::row()
        .align_items(sigil::prelude::AlignItems::Stretch)
        .fill_width()
        .fill_height()
        .gap(Space::None)
        .background(canvas::palette().base)
        .child(surface);
    match layout.secondary {
        Some(pane) if side_cols > 0 => row.child(side(app, pane, side_cols)),
        Some(_) | None => row,
    }
}

fn side(app: App, pane: Pane, cols: u16) -> NodeBuilder {
    container()
        .fill_height()
        .shrink(0.0)
        .min_width(0.0)
        .min_height(0.0)
        .children_reactive(move || vec![panes::body(&app, pane, cols)])
}

pub(crate) fn editor(app: &App, editing: bool) -> TextEditBuilder {
    let (value, caret) = if editing {
        (app.view_text, app.view_caret)
    } else {
        (app.draft, app.caret)
    };
    let editor = input_keys(app, text_edit(value))
        .caret_position(caret)
        .focus_when(app.editor_focus);
    if editing {
        return editor;
    }
    let changed = app.clone();
    editor.state(app.editor.clone()).change(move |draft| {
        changed.pane_focus.set(false);
        changed.change(draft);
    })
}

pub(super) fn terminal_editor(app: App) -> impl FnOnce() -> NodeBuilder {
    let submit = app.clone();
    let editor = editor_layout(
        editor(&app, false).content_padding(0.0, 0.0).chrome(false),
        app.viewport.peek().1,
    )
    .placeholder("")
    .submit(move |_| submit.submit());
    move || {
        if untrack(|| app.overlay().is_none() && app.note_target.peek().is_none()) {
            editor.autofocus().into()
        } else {
            editor.into()
        }
    }
}

fn chat_surface(app: App, cols: u16) -> NodeBuilder {
    let editor = terminal_editor(app.clone());
    let surface = if app.inline {
        chat::inline_layer(app, cols, editor)
    } else if app.view_edit.get() {
        chat::layer(app, cols, container())
    } else {
        chat::layer(app, cols, editor)
    };
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .fill_height()
        .grow(1.0)
        .min_width(0.0)
        .min_height(0.0)
        .background(canvas::palette().base)
        .child(surface)
}

fn editor_layout<W: Widget>(node: W, rows: u16) -> W {
    let cell = canvas::cell().h;
    let visible = (rows / 2).saturating_sub(2).max(1);
    node.fill_width()
        .height(Length::Auto)
        .min_width(0.0)
        .min_height(cell)
        .max_width(Length::Auto)
        .max_height(f32::from(visible) * cell)
        .shrink(0.0)
}
