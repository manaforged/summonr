use super::*;
use sigil::prelude::Dimension;

pub(super) fn layer(app: App) -> NodeBuilder {
    let (cols, _rows) = app.viewport.get();
    let layout = app.layout.get();
    let (chat_cols, side_cols) = layout.split(cols);
    let surface = chat_surface(app.clone(), chat_cols);
    match layout.secondary {
        Some(pane) if side_cols > 0 => container()
            .layout(LayoutStyle::flex_row().fill_width().fill_height().gap(0.0))
            .style(Style::new().bg(SCRIM))
            .child(surface)
            .child(super::super::panes::body(&app, pane, side_cols)),
        _ => surface,
    }
}

fn chat_surface(app: App, cols: u16) -> NodeBuilder {
    let submit = app.clone();
    let changed = app.clone();
    let editor = text_edit(app.draft)
        .caret_position(app.caret)
        .state(app.editor.clone())
        .layout(editor_layout(app.viewport.peek().1))
        .placeholder("")
        .change(move |draft| {
            changed.pane_focus.set(false);
            changed.change(draft);
        })
        .submit(move |_| submit.submit());
    let surface = if app.view_edit.get() {
        super::super::chat::layer(app, cols, container())
    } else {
        super::super::chat::layer(app, cols, editor)
    };
    container()
        .layout(
            LayoutStyle::block()
                .width(f32::from(cols) * super::super::canvas::cell().w)
                .fill_height(),
        )
        .style(Style::new().bg(SCRIM))
        .child(surface)
}

fn editor_layout(rows: u16) -> LayoutStyle {
    let visible = (rows / 2).saturating_sub(2).clamp(1, 8);
    LayoutStyle::flex_column()
        .fill_width()
        .min_size(0.0, 0.0)
        .max_size(
            Dimension::Auto,
            f32::from(visible) * super::super::canvas::cell().h,
        )
}

pub(super) fn theme(_name: &str) -> Theme {
    let mut theme = Theme::dark();
    theme.register_token(
        "accent",
        ThemeValue::Color(super::super::canvas::palette().accent),
    );
    theme
}
