use super::*;
use sigil::layout::Dimension;
use sigil::reactive::untrack;

pub(super) fn layer(app: App) -> NodeBuilder {
    let (cols, _rows) = app.viewport.get();
    let layout = app.layout.get();
    let (chat_cols, side_cols) = if app.inline {
        (cols, 0)
    } else {
        layout.split(cols)
    };
    let surface = chat_surface(app.clone(), chat_cols);
    match layout.secondary {
        Some(pane) if side_cols > 0 => container()
            .layout(LayoutStyle::flex_row().fill_width().fill_height().gap(0.0))
            .style(Style::new().bg(canvas::palette().base))
            .child(surface)
            .child(panes::body(&app, pane, side_cols)),
        _ => surface,
    }
}

fn chat_surface(app: App, cols: u16) -> NodeBuilder {
    let submit = app.clone();
    let changed = app.clone();
    let editor = text_edit(app.draft)
        .caret_position(app.caret)
        .state(app.editor.clone())
        .content_padding(0.0, 0.0)
        .chrome(false)
        .layout(editor_layout(app.viewport.peek().1))
        .placeholder("")
        .change(move |draft| {
            changed.pane_focus.set(false);
            changed.change(draft);
        })
        .submit(move |_| submit.submit())
        .focus_when(app.editor_focus);
    let focus = app.clone();
    let editor = move || {
        if untrack(|| focus.overlay().is_none()) {
            editor.autofocus()
        } else {
            editor
        }
    };
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
        .style(Style::new().bg(canvas::palette().base))
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

pub(super) fn theme(_name: &str) -> Theme {
    let palette = canvas::palette();
    let mut theme = Theme::dark();
    for (token, color) in tokens(palette) {
        theme.register_token(token, ThemeValue::Color(color));
    }
    theme
}

fn tokens(palette: Palette) -> [(&'static str, Rgba8); 31] {
    [
        ("accent", palette.accent),
        (
            "accent-5",
            canvas::blend(palette.base, palette.accent, 0.18),
        ),
        ("accent-11", palette.accent),
        ("surface", palette.base),
        ("surface-hover", palette.tool_bg),
        ("surface-pressed", palette.user_bg),
        ("on-surface", palette.text),
        ("border", palette.dim),
        ("scrim", canvas::blend(palette.black, palette.base, 0.5)),
        ("field", palette.tool_bg),
        ("gray-1", palette.black),
        ("gray-2", palette.base),
        ("gray-3", palette.tool_bg),
        ("gray-4", palette.code_bg),
        ("gray-5", palette.user_bg),
        ("gray-6", palette.code_bg),
        ("gray-7", palette.dim),
        ("gray-8", palette.gray),
        ("gray-11", palette.dim),
        ("gray-12", palette.text),
        (
            "success-3",
            canvas::blend(palette.tool_bg, palette.green, 0.2),
        ),
        (
            "success-5",
            canvas::blend(palette.tool_bg, palette.green, 0.45),
        ),
        ("success-9", palette.green),
        ("success-11", palette.green),
        ("warn-11", palette.yellow),
        (
            "error-3",
            canvas::blend(palette.tool_bg, palette.error, 0.25),
        ),
        (
            "error-5",
            canvas::blend(palette.tool_bg, palette.error, 0.45),
        ),
        ("error-11", palette.error),
        ("info-3", canvas::blend(palette.tool_bg, palette.cyan, 0.2)),
        ("info-11", palette.cyan),
        (
            "accent-hover",
            canvas::blend(palette.base, palette.accent, 0.8),
        ),
    ]
}
