use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, Role, WidgetLayoutExt,
    classes, container, label, modal, signal, text_edit, virtual_list,
};

use super::super::app::App;

enum ModelRow {
    Header(&'static str),
    Seat(usize),
}

fn list_rows(count: usize, recent: usize, sections: bool) -> Vec<ModelRow> {
    if !sections {
        return (0..count).map(ModelRow::Seat).collect();
    }
    let mut rows = Vec::with_capacity(count + 2);
    rows.push(ModelRow::Header("recent"));
    rows.extend((0..recent).map(ModelRow::Seat));
    if recent < count {
        rows.push(ModelRow::Header("providers"));
        rows.extend((recent..count).map(ModelRow::Seat));
    }
    rows
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    container().layout(LayoutStyle::block()).child(
        modal(app.editing_model).title("models").trap(true).child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .gap(6.0)
                        .fill_width()
                        .fill_height(),
                )
                .children_reactive(move || {
                    super::super::view::watch(&paint);
                    let typing = paint.clone();
                    let error = paint.model_error.get();
                    let loading = paint.model_loading.get();
                    let choices = paint.filtered_models();
                    let sections =
                        paint.model_draft.get().is_empty() && paint.model_recent.get() > 0;
                    let recent = paint.model_recent.get().min(choices.len());
                    let rows = list_rows(choices.len(), recent, sections);
                    let selected = paint.model_sel.get().min(choices.len().saturating_sub(1));
                    let selected_row = rows
                        .iter()
                        .position(|row| matches!(row, ModelRow::Seat(index) if *index == selected))
                        .unwrap_or(0);
                    let current: Vec<bool> = {
                        let model = paint.model.borrow();
                        choices
                            .iter()
                            .map(|choice| model.is_model(choice))
                            .collect()
                    };
                    let mut items: Vec<NodeBuilder> = vec![
                        container().child(
                            text_edit(paint.model_draft)
                                .caret_position(paint.model_caret)
                                .placeholder("Filter models")
                                .change(move |draft| typing.replace_model_draft(draft.to_string()))
                                .height(32.0),
                        ),
                    ];
                    if loading {
                        items.push(container().child(
                            label("loading models…").class(classes!("text-xs text-gray-11")),
                        ));
                    }
                    if !error.is_empty() {
                        items.push(
                            container().child(
                                label(error.replace('\n', " "))
                                    .class(classes!("text-xs text-error-11")),
                            ),
                        );
                    }
                    if choices.is_empty() && !loading {
                        items.push(
                            container().child(
                                label("No matches. Change the filter.")
                                    .class(classes!("text-xs text-warn-11")),
                            ),
                        );
                    }
                    if !choices.is_empty() {
                        let source = paint.clone();
                        let lookup = choices.clone();
                        items.push(
                            container().layout(LayoutStyle::block().fill_width()).child(
                                virtual_list(rows, move |_, row: &ModelRow| match row {
                                    ModelRow::Header(name) => container().child(
                                        label(name.to_string())
                                            .class(classes!("text-xs text-gray-11")),
                                    ),
                                    ModelRow::Seat(index) => entry(
                                        source.clone(),
                                        &lookup[*index],
                                        current[*index],
                                        *index,
                                        *index == selected,
                                    ),
                                })
                                .row_height(22.0)
                                .viewport_fill()
                                .scroll_into_view(selected_row),
                            ),
                        );
                    }
                    items
                }),
        ),
    )
}

fn entry(app: App, choice: &str, current: bool, index: usize, active: bool) -> NodeBuilder {
    let choose = app.clone();
    let (marker, title) = if active {
        ('›', choice)
    } else {
        (' ', choice)
    };
    let title = match title.split_once('\t') {
        Some((name, model)) => format!("{marker} {name}  {model}"),
        None => format!("{marker} {title}"),
    };
    container()
        .role(Role::Button)
        .label(title.clone())
        .focusable(true)
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(22.0)
                .padding_x(8.0)
                .gap(6.0),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(active), "bg-gray-5")
        .pointer_down(move |event| {
            choose.model_sel.set(index);
            choose.commit_model_edit();
            event.stop_propagation();
        })
        .child(
            label(if current { "●" } else { " " }.to_string())
                .class(classes!("text-xs text-success-11")),
        )
        .child(label(title).class(classes!("text-sm text-gray-12")))
}
