use super::{App, Model, composer_hint, suggest};
use crate::tui::sigil_view::canvas::GUTTER;
use sigil::prelude::{
    AlignItems, IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Overflow,
    Position, Tone, classes, container, divider_horizontal, label, node, notice_band,
};

pub(in crate::tui::sigil_view::chat) fn widget<E: IntoNode + 'static>(
    app: App,
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
        let model = app.model.get();
        let path = model.view.as_ref().map_or("", |view| view.path.as_str());
        editor_area = editor_area
            .child(notice_band(
                format!("editing {path} · ctrl+s save · esc leave"),
                Tone::Info,
            ))
            .child(editor);
    } else {
        editor_area = editor_area.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .min_size(0.0, 0.0)
                        .align_items(AlignItems::Start)
                        .gap(8.0),
                )
                .child(label("❯").class(classes!("text-xs text-gray-12")))
                .child(
                    container()
                        .layout(
                            LayoutStyle::flex_column()
                                .grow(1.0)
                                .min_size(0.0, 0.0)
                                .overflow_y(Overflow::Hidden),
                        )
                        .child(editor),
                ),
        );
    }
    let inner_width = width.saturating_sub(8);
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .padding_top(8.0)
                .padding_x(GUTTER - 8.0)
                .gap(0.0),
        )
        .child(suggest::widget(app.clone(), width))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .min_size(0.0, 0.0)
                        .padding_x(8.0)
                        .gap(0.0),
                )
                .class(classes!("bg-surface"))
                .child(header_row(app.clone()))
                .child(editor_area)
                .child(hint_row(app.clone(), inner_width)),
        )
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
            let model = app.model.get();
            vec![mode_label(&model), rule(), model_label(&model)]
        })
}

fn hint_row(app: App, width: usize) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .children_reactive(move || {
            let _select = app.select.get();
            let model = app.model.get();
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
                    width,
                    &options,
                    select.as_deref(),
                )
                .trim()
                .to_string()
            };
            vec![
                node(rule()),
                node(label(hint).class(classes!("text-xs text-gray-11"))),
                node(rule()),
            ]
        })
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
