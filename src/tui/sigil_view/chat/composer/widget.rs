use std::cell::Cell;
use std::rc::Rc;

use super::{App, Model, composer_hint, suggest};
use crate::tui::sigil_view::canvas::GUTTER;
use sigil::events::NodeEventsExt;
use sigil::prelude::{
    AlignItems, Cx, IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Overflow,
    Position, Tone, classes, container, derived_eq, divider_horizontal, label, node, notice_band,
    request_focus,
};

pub(in crate::tui::sigil_view::chat) fn widget<E: IntoNode + 'static>(
    app: App,
    width: usize,
    editor: E,
) -> NodeBuilder {
    let editing = app.view_edit.peek();
    let editor_node = Rc::new(Cell::new(None));
    let mount = editor_node.clone();
    let editor = move |cx: &mut Cx| {
        let id = cx.add(editor);
        mount.set(Some(id));
        id
    };
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
        .pointer_down(move |_| {
            if let Some(id) = editor_node.get() {
                request_focus(id);
            }
        })
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

type ModeText = (String, u8);

fn mode_text(model: &Model) -> ModeText {
    let class: usize = if model.snapshot.plan {
        0
    } else if model.snapshot.work {
        1
    } else if model.snapshot.goal {
        2
    } else {
        3
    };
    let mode = ["plan", "work", "goal", "auto"][class];
    let glyph = ["◇", "▣", "◎", "●"][class];
    (format!("{glyph} {mode}"), class as u8)
}

fn model_text(model: &Model) -> String {
    let suffix = model
        .snapshot
        .effort
        .filter(|effort| *effort != crate::provider::Effort::None)
        .map_or(String::new(), |effort| format!(" ({})", effort.as_str()));
    format!("{}{suffix}", model.snapshot.model)
}

fn header_row(app: App) -> NodeBuilder {
    let update = app.clone();
    let labels = derived_eq(move || {
        let model = update.model.get();
        (mode_text(&model), model_text(&model))
    });
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .children_reactive(move || {
            let (mode, model) = labels.get();
            vec![mode_label(mode), rule(), model_label(model)]
        })
}

fn hint_row(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let hint = derived_eq(move || {
        let select = update.select.get();
        let model = update.model.get();
        let options = model.prefs.hotkeys.label(crate::prefs::Hotkey::Options);
        let select = select.then(|| model.prefs.hotkeys.label(crate::prefs::Hotkey::Select));
        if update.view_edit.get() {
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
        }
    });
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .children_reactive(move || {
            vec![
                node(rule()),
                node(label(hint.get()).class(classes!("text-xs text-gray-11"))),
                node(rule()),
            ]
        })
}

fn mode_label(mode: ModeText) -> NodeBuilder {
    let (text, class) = mode;
    let title = label(text);
    let title = if class == 0 {
        title.class(classes!("text-xs text-accent"))
    } else if class == 1 {
        title.class(classes!("text-xs text-warn-11"))
    } else if class == 2 {
        title.class(classes!("text-xs text-error-11"))
    } else {
        title.class(classes!("text-xs text-success-11"))
    };
    container().child(title)
}

fn model_label(model: String) -> NodeBuilder {
    container().child(label(model).class(classes!("text-xs text-gray-11")))
}
