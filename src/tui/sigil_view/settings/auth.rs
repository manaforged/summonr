use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, Role, WidgetLayoutExt,
    classes, container, label, modal, signal, text_edit, virtual_list,
};

use super::super::app::App;

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    container().layout(LayoutStyle::block()).child(
        modal(app.auth_open)
            .title("authentication")
            .trap(true)
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .gap(8.0)
                            .fill_width()
                            .fill_height(),
                    )
                    .children_reactive(move || {
                        super::super::view::watch(&paint);
                        if paint.auth_entering.get() {
                            return entering(&paint);
                        }
                        choices(&paint)
                    }),
            ),
    )
}

fn entering(app: &App) -> Vec<NodeBuilder> {
    let typing = app.clone();
    let save = app.clone();
    let choices = app.auth_choices.get();
    let selected = app.auth_sel.get().min(choices.len().saturating_sub(1));
    let mut items = Vec::new();
    if let Some(choice) = choices.get(selected) {
        items.push(
            container().child(label(choice.name.clone()).class(classes!("text-sm text-gray-12"))),
        );
    }
    items.push(container().child(label("API key").class(classes!("text-xs text-gray-11"))));
    items.push(
        container().child(
            text_edit(app.auth_key)
                .caret_position(app.auth_caret)
                .change(move |key| typing.replace_auth_key(key.to_string()))
                .submit(move |_| save.commit_auth())
                .height(32.0),
        ),
    );
    items.push(
        container()
            .child(label("Saves in the credential store").class(classes!("text-xs text-gray-11"))),
    );
    items
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
    let lookup = rows
        .iter()
        .map(|choice| (choice.name.clone(), choice.state()))
        .collect::<Vec<_>>();
    vec![
        container().layout(LayoutStyle::block().fill_width()).child(
            virtual_list(lookup, move |index, (name, state): &(String, String)| {
                entry(source.clone(), name, state, index, index == selected)
            })
            .row_height(24.0)
            .viewport_fill()
            .scroll_into_view(selected),
        ),
    ]
}

fn entry(app: App, name: &str, state: &str, index: usize, active: bool) -> NodeBuilder {
    let choose = app.clone();
    let marker = if active { '›' } else { ' ' };
    container()
        .role(Role::Button)
        .label(name.to_string())
        .focusable(true)
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(24.0)
                .padding_x(8.0)
                .gap(8.0),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(active), "bg-gray-5")
        .pointer_down(move |event| {
            choose.auth_sel.set(index);
            choose.commit_auth();
            event.stop_propagation();
        })
        .child(label(format!("{marker} {name}")).class(classes!("text-sm text-gray-12")))
        .child(label(state.to_string()).class(classes!("text-xs text-gray-11")))
}
