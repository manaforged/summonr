use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, WidgetLayoutExt,
    classes, container, label, text_field, virtual_list,
};

use super::prelude::*;
use super::{App, canvas, watch};

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    shell(
        app.auth_open,
        "Authentication",
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .children_reactive(move || {
                watch(&paint);
                if paint.auth_entering.get() {
                    return entering(&paint);
                }
                choices(&paint)
            }),
    )
}

fn chip(state: &str) -> String {
    if state.contains("missing") {
        "not set".to_string()
    } else if state.contains("required") {
        "sign in".to_string()
    } else {
        format!("✓ {state}")
    }
}

fn choices(app: &App) -> Vec<NodeBuilder> {
    let source = app.clone();
    let rows = app.auth_choices.get();
    let selected = app.auth_sel.get().min(rows.len().saturating_sub(1));
    if rows.is_empty() {
        return vec![
            container().child(
                label("No providers to sign in to.").class(classes!("text-xs text-gray-11")),
            ),
        ];
    }
    let (_, term_rows) = app.viewport.get();
    let visible = usize::from(term_rows.saturating_sub(12)).clamp(6, 14);
    let lookup = rows
        .iter()
        .map(|choice| (choice.name.clone(), choice.state()))
        .collect::<Vec<_>>();
    let mut items = vec![
        container().layout(LayoutStyle::block().fill_width()).child(
            virtual_list(lookup, move |index, (name, state): &(String, String)| {
                entry(source.clone(), name, state, index, index == selected)
            })
            .row_height(canvas::overlay_list_row())
            .height(canvas::overlay_list_row() * visible as f32)
            .viewport_height(canvas::overlay_list_row() * visible as f32)
            .follow(app.auth_sel),
        ),
    ];
    if let Some(choice) = rows.get(selected) {
        items.push(group(format!(
            "{} — {}",
            choice.name,
            canvas::ellipsize(&choice.setup, 60)
        )));
        items.push(pick(
            false,
            false,
            "sign in with",
            match choice.flow {
                crate::LoginFlow::Browser => "browser sign-in — opens a tab",
                crate::LoginFlow::Device => "device login — shows a code",
                crate::LoginFlow::Key => "api key — paste to store",
            },
        ));
    }
    items.push(hint(
        "↑↓ move · ↵ sign in · esc close",
        format!("{} / {}", selected + 1, rows.len()),
    ));
    items
}

fn entry(app: App, name: &str, state: &str, index: usize, active: bool) -> NodeBuilder {
    let choose = app.clone();
    pick(active, false, name.to_string(), chip(state)).pointer_down(move |event| {
        choose.auth_sel.set(index);
        choose.commit_auth();
        event.stop_propagation();
    })
}

fn entering(app: &App) -> Vec<NodeBuilder> {
    let typing = app.clone();
    let save = app.clone();
    let count = app.clone();
    let choices = app.auth_choices.get();
    let selected = app.auth_sel.get().min(choices.len().saturating_sub(1));
    let provider = choices
        .get(selected)
        .map(|choice| choice.name.clone())
        .unwrap_or_default();
    let (cols, _) = app.viewport.get();
    let span = (canvas::dialog_width(cols, 48.0) / canvas::cell().w) as usize;
    let guide = canvas::ellipsize(
        &format!("paste the {provider} key — it is masked before it is stored"),
        span.saturating_sub(6),
    );
    let items = vec![
        group(guide),
        container().child(
            text_field(app.auth_key)
                .placeholder("paste or type the api key")
                .focus_when(app.auth_focus)
                .password()
                .change(move |key| typing.replace_auth_key(key.to_string()))
                .submit(move |_| save.commit_auth())
                .height(canvas::overlay_field()),
        ),
        container().children_reactive(move || {
            let characters = count.auth_key.get().chars().count();
            vec![pick(
                false,
                false,
                format!("{characters} characters"),
                "↵ save",
            )]
        }),
        hint("↵ save · esc back", provider),
    ];
    items
}
