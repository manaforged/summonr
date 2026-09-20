use sigil::prelude::{
    AlignItems, IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Position, Tone,
    classes, container, divider_horizontal, label, notice_band,
};

use super::{App, Model, composer_hint, suggest};

pub(in crate::tui::sigil_view::chat) fn widget<E: IntoNode + 'static>(
    app: App,
    full_width: usize,
    width: usize,
    editor: E,
) -> NodeBuilder {
    let editing = app.view_edit.peek();
    let mut editor_area = container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .min_size(0.0, 0.0)
            .position(Position::Relative),
    );
    if editing {
        let model = app.model.borrow();
        let path = model.view.as_ref().map_or("", |view| view.path.as_str());
        editor_area = editor_area
            .child(notice_band(
                format!("editing {path} · ctrl+s save · esc leave"),
                Tone::Info,
            ))
            .child(editor);
    } else {
        editor_area = editor_area.child(editor);
    }
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .padding_x(16.0)
                .gap(0.0),
        )
        .child(suggest::widget(app.clone(), width, full_width))
        .child(header_row(app.clone()))
        .child(editor_area)
        .child(hint_row(app.clone()))
}

fn rule() -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_row().grow(1.0).min_size(0.0, 0.0))
        .child(divider_horizontal())
}

fn header_row(app: App) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .children_reactive(move || {
            let _panes = app.panes.get();
            let model = app.model.borrow();
            vec![
                container()
                    .layout(
                        LayoutStyle::flex_row()
                            .fill_width()
                            .align_items(AlignItems::Center)
                            .gap(8.0),
                    )
                    .child(mode_label(&model))
                    .child(rule())
                    .child(model_label(&model)),
            ]
        })
}

fn hint_row(app: App) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .children_reactive(move || {
            let _panes = app.panes.get();
            let _select = app.select.get();
            let model = app.model.borrow();
            let options = model.prefs.hotkeys.label(crate::prefs::Hotkey::Options);
            let select = app
                .select
                .peek()
                .then(|| model.prefs.hotkeys.label(crate::prefs::Hotkey::Select));
            let hint = if app.view_edit.get() {
                "ctrl+s save · esc leave".to_string()
            } else {
                composer_hint(
                    model.busy_since.map(|(activity, _)| activity),
                    width_of(&app),
                    &options,
                    select.as_deref(),
                )
                .trim()
                .to_string()
            };
            vec![
                container()
                    .layout(
                        LayoutStyle::flex_row()
                            .fill_width()
                            .align_items(AlignItems::Center)
                            .gap(8.0),
                    )
                    .child(rule())
                    .child(label(hint).class(classes!("text-xs text-gray-11"))),
            ]
        })
}

fn width_of(app: &App) -> usize {
    usize::from(app.viewport.peek().0).saturating_sub(18)
}

fn mode_label(model: &Model) -> NodeBuilder {
    let (glyph, mode) = if model.snapshot.plan {
        ("◇", "plan")
    } else if model.snapshot.work {
        ("▣", "work")
    } else if model.snapshot.goal {
        ("◎", "goal")
    } else {
        ("●", "auto")
    };
    let title = label(format!("{glyph} {mode}"));
    let title = if model.snapshot.plan {
        title.class(classes!("text-xs text-accent"))
    } else if model.snapshot.work {
        title.class(classes!("text-xs text-warn-11"))
    } else if model.snapshot.goal {
        title.class(classes!("text-xs text-error-11"))
    } else {
        title.class(classes!("text-xs text-success-11"))
    };
    container().child(title)
}

fn model_label(model: &Model) -> NodeBuilder {
    let suffix = model
        .snapshot
        .effort
        .filter(|effort| *effort != crate::provider::Effort::None)
        .map_or(String::new(), |effort| format!(" ({})", effort.as_str()));
    container().child(
        label(format!("{}{suffix}", model.snapshot.model)).class(classes!("text-xs text-gray-11")),
    )
}
