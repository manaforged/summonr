use super::*;
use crate::tui::sigil_view::ui::input_keys;
use sigil::prelude::Dimension;
use sigil::prelude::Paint;
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
    let row = container()
        .layout(LayoutStyle::flex_row().fill_width().fill_height().gap(0.0))
        .bg(canvas::palette().base)
        .child(surface);
    match layout.secondary {
        Some(pane) if side_cols > 0 => row.child(side(app, pane, side_cols)),
        Some(_) | None => row,
    }
}

fn side(app: App, pane: Pane, cols: u16) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::block()
                .fill_height()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || vec![panes::body(&app, pane, cols)])
}

pub(in crate::tui::sigil_view) fn editor(app: &App, editing: bool) -> TextEditBuilder {
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
    let editor = editor(&app, false)
        .content_padding(0.0, 0.0)
        .chrome(false)
        .layout(editor_layout(app.viewport.peek().1))
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
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .bg(canvas::palette().base)
        .child(surface)
}

fn editor_layout(rows: u16) -> LayoutStyle {
    let cell = canvas::cell().h;
    let visible = (rows / 2).saturating_sub(2).max(1);
    LayoutStyle::flex_column()
        .fill_width()
        .height(Dimension::Auto)
        .min_size(0.0, cell)
        .max_size(Dimension::Auto, f32::from(visible) * cell)
        .shrink(0.0)
}
