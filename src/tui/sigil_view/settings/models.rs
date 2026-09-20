use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, WidgetLayoutExt, container,
    text_field, virtual_list,
};

use super::prelude::*;
use super::{App, canvas, watch};

enum ModelRow {
    Header(String),
    Seat(usize),
}

fn provider(choice: &str) -> &str {
    choice.split_once('\t').map_or("", |(name, _)| name)
}

fn list_rows(
    choices: &[String],
    recent: usize,
    sections: bool,
    groups: &std::collections::BTreeMap<String, String>,
) -> Vec<ModelRow> {
    if !sections {
        return (0..choices.len()).map(ModelRow::Seat).collect();
    }
    let mut rows = Vec::with_capacity(choices.len() + 4);
    rows.push(ModelRow::Header("current".to_string()));
    rows.extend((0..recent).map(ModelRow::Seat));
    let mut heading = String::new();
    for (index, choice) in choices.iter().enumerate().skip(recent) {
        let name = provider(choice);
        if name != heading {
            heading = name.to_string();
            rows.push(ModelRow::Header(match groups.get(name) {
                Some(state) => format!("{name} · {state}"),
                None => name.to_string(),
            }));
        }
        rows.push(ModelRow::Seat(index));
    }
    rows
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    shell(
        app.editing_model,
        "Models",
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .children_reactive(move || {
                watch(&paint);
                let typing = paint.clone();
                let error = paint.model_error.get();
                let loading = paint.model_loading.get();
                let choices = paint.filtered_models();
                let sections = paint.model_draft.get().is_empty() && paint.model_recent.get() > 0;
                let recent = paint.model_recent.get().min(choices.len());
                let groups = paint.model_groups.get();
                let rows = list_rows(&choices, recent, sections, &groups);
                let visible = canvas::overlay_shown(rows.len());
                let (cols, _) = paint.viewport.get();
                let span = (canvas::dialog_width(cols, 64.0) / canvas::cell().w) as usize;
                let error = canvas::ellipsize(&error.replace('\n', " "), span.saturating_sub(6));
                let selected = paint.model_sel.get().min(choices.len().saturating_sub(1));
                let current: Vec<bool> = {
                    let model = paint.model.borrow();
                    choices
                        .iter()
                        .map(|choice| model.is_model(choice))
                        .collect()
                };
                let mut items: Vec<NodeBuilder> = vec![
                    container().child(
                        text_field(paint.model_draft)
                            .placeholder("Filter models")
                            .focus_when(paint.models_focus)
                            .change(move |draft| typing.replace_model_draft(draft.to_string()))
                            .submit(|_| {})
                            .height(canvas::overlay_field()),
                    ),
                ];
                if loading {
                    items.push(group("loading models…"));
                }
                if !error.is_empty() {
                    items.push(band(error, true));
                }
                if choices.is_empty() && !loading {
                    items.push(band("No matches. Change the filter.", true));
                }
                if !choices.is_empty() {
                    let source = paint.clone();
                    let lookup = choices.clone();
                    items.push(
                        container().layout(LayoutStyle::block().fill_width()).child(
                            virtual_list(rows, move |_, row: &ModelRow| match row {
                                ModelRow::Header(name) => group(name.clone()),
                                ModelRow::Seat(index) => entry(
                                    source.clone(),
                                    &lookup[*index],
                                    current[*index],
                                    *index,
                                    *index == selected,
                                    sections && *index >= recent,
                                ),
                            })
                            .row_height(canvas::overlay_list_row())
                            .height(canvas::overlay_list_row() * visible as f32)
                            .viewport_height(canvas::overlay_list_row() * visible as f32)
                            .follow(paint.model_sel),
                        ),
                    );
                }
                if let Some(about) = choices
                    .get(selected)
                    .and_then(|choice| paint.model_facts.get().get(choice).cloned())
                    .and_then(|fact| fact.about)
                {
                    items.push(group(canvas::ellipsize(&about, span.saturating_sub(6))));
                }
                items.push(hint(
                    "↑↓ move · ↵ use · ⌃S default · ⌃R refresh · esc",
                    format!("{} / {}", selected + 1, choices.len()),
                ));
                items
            }),
    )
}

fn entry(
    app: App,
    choice: &str,
    current: bool,
    index: usize,
    active: bool,
    grouped: bool,
) -> NodeBuilder {
    let choose = app.clone();
    let (endpoint, model) = match choice.split_once('\t') {
        Some((endpoint, model)) => (endpoint.to_string(), model.to_string()),
        None => (String::new(), choice.to_string()),
    };
    let facts = app.model_facts.get();
    let found = facts.get(choice);
    let model = found.map_or(model, |fact| fact.label().to_string());
    let detail = [
        (!grouped).then_some(endpoint),
        found.map(crate::catalog::Model::window),
        found.map(|fact| fact.cost.label().to_string()),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    pick(active, current, model, detail).pointer_down(move |event| {
        choose.model_sel.set(index);
        choose.commit_model_edit();
        event.stop_propagation();
    })
}
