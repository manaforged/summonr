use sigil::prelude::{
    AlignItems, JustifyContent, Key, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt,
    NodeStyleExt, Overflow, WidgetLayoutExt, button, classes, container, label, request_focus,
    text_edit, text_field,
};

use super::super::app::App;
use super::{hit, spacer};
use crate::prefs::Hotkey;
use crate::workflow::Role as WorkflowRole;

const WIDTH: f32 = 360.0;

pub(super) fn auth(app: App) -> NodeBuilder {
    let escape = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill()
                .justify_content(JustifyContent::Center),
        )
        .class(classes!("bg-gray-1"))
        .key_capture(Key::Escape, move |event| {
            escape.cancel_auth();
            event.stop_propagation();
        })
        .children_reactive(move || {
            let entering = app.auth_entering.get();
            let choices = app.auth_choices.get();
            let selected = app.auth_sel.peek();
            let back = app.clone();
            let mut panel = pane()
                .child(label("Authentication").class(classes!("text-sm font-medium text-gray-12")));
            if entering {
                if let Some(choice) = choices.get(selected) {
                    let save = app.clone();
                    let submit = app.clone();
                    panel = panel
                        .child(label(choice.name.as_str()).class(classes!("text-sm text-gray-12")))
                        .child(
                            container()
                                .layout(LayoutStyle::block().fill_width().height(34.0))
                                .child(
                                    text_field(app.auth_key)
                                        .password()
                                        .placeholder("API key")
                                        .submit(move |_| submit.commit_auth())
                                        .height(34.0)
                                        .class(classes!(
                                            "rounded-md border border-gray-6 bg-gray-1"
                                        )),
                                )
                                .on_added(|cx, id| {
                                    if let Some(field) = cx.first_child(id) {
                                        request_focus(field);
                                    }
                                }),
                        )
                        .child(
                            label("Saves in the credential store.")
                                .class(classes!("text-xs text-gray-11")),
                        )
                        .child(
                            button("Save API key")
                                .height(32.0)
                                .activate(move || save.commit_auth()),
                        );
                }
            } else {
                for (index, choice) in choices.iter().enumerate() {
                    let pick = app.clone();
                    let action = if choice.device_login && choice.configured {
                        "Sign in again"
                    } else if choice.device_login {
                        "Sign in"
                    } else if choice.configured {
                        "Replace key"
                    } else {
                        "Add key"
                    };
                    panel = panel.child(
                        container()
                            .layout(LayoutStyle::block().fill_width().height(32.0))
                            .child(
                                button(format!("{} · {action}", choice.name))
                                    .ghost()
                                    .height(28.0)
                                    .accessible(choice.status())
                                    .activate(move || {
                                        pick.auth_sel.set(index);
                                        pick.commit_auth();
                                    }),
                            )
                            .on_added(move |cx, id| {
                                if index == selected
                                    && let Some(button) = cx.first_child(id)
                                {
                                    request_focus(button);
                                }
                            }),
                    );
                }
                if choices.is_empty() {
                    panel = panel.child(
                        label("No providers available.").class(classes!("text-sm text-error-11")),
                    );
                }
            }
            vec![
                panel
                    .child(
                        button(if entering { "Back" } else { "Close" })
                            .ghost()
                            .height(32.0)
                            .activate(move || back.cancel_auth()),
                    )
                    .child(
                        label("Tab selects · Enter activates · Esc backs")
                            .class(classes!("text-xs text-gray-11")),
                    ),
            ]
        })
}

pub(super) fn main(app: App) -> NodeBuilder {
    let close = app.clone();
    let rows = super::super::settings::setting_rows(&app);
    let mut panel = pane().child(header("Options", move || close.toggle_options()));
    let mut group = "";
    for (index, row) in rows.into_iter().enumerate() {
        if row.group != group {
            group = row.group;
            panel = panel.child(section(group));
        }
        let change = app.clone();
        let selected = app.settings_sel.peek() == index;
        panel = panel.child(
            hit(row.label.clone(), selected, move || {
                change.settings_sel.set(index);
                change.adjust_setting();
            })
            .child(label(row.label).class(classes!("text-sm text-gray-12")))
            .child(spacer())
            .child(
                label(row.value)
                    .ellipsize()
                    .layout(
                        LayoutStyle::block()
                            .grow(1.0)
                            .max_size(190.0, f32::INFINITY)
                            .min_size(0.0, 16.0),
                    )
                    .class(classes!("text-xs font-mono text-gray-11")),
            ),
        );
    }
    panel.child(
        label("Click a row or use ↑↓ and ←→. Enter opens lists.")
            .class(classes!("text-xs text-gray-11")),
    )
}

pub(super) fn hotkeys(app: App) -> NodeBuilder {
    let back = app.clone();
    let reset = app.clone();
    let selected = app.hotkey_sel.peek().min(Hotkey::ALL.len() - 1);
    let mut panel = pane()
        .child(header("Hotkeys", move || back.close_hotkeys()))
        .child(
            label("Choose an action, then press Ctrl+letter. Conflicts swap bindings.")
                .class(classes!("text-xs text-gray-11")),
        );
    for (index, action) in Hotkey::ALL.into_iter().enumerate() {
        let capture = app.clone();
        let active = selected == index;
        let binding = if active && app.hotkey_recording.peek() {
            "press Ctrl+letter".to_string()
        } else {
            app.model.borrow().prefs.hotkeys.label(action)
        };
        panel = panel.child(
            hit(action.name().to_string(), active, move || {
                capture.hotkey_recording.set(false);
                capture.hotkey_note.set(String::new());
                capture.hotkey_sel.set(index);
                capture.edit_hotkey();
            })
            .child(label(action.name()).class(classes!("text-sm text-gray-12")))
            .child(spacer())
            .child(label(binding).class(classes!("text-xs font-mono text-gray-11"))),
        );
    }
    let note = app.hotkey_note.peek();
    if !note.is_empty() {
        panel = panel.child(label(note).class(classes!("text-xs text-error-11")));
    }
    panel.child(
        hit("Reset hotkeys".to_string(), false, move || {
            reset.reset_hotkeys()
        })
        .child(label("Reset defaults").class(classes!("text-xs text-gray-11"))),
    )
}

pub(super) fn models(app: App) -> NodeBuilder {
    let back = app.clone();
    let changed = app.clone();
    let submit = app.clone();
    let mut panel = pane()
        .child(header("Models", move || back.cancel_model_edit()))
        .child(
            text_edit(app.model_draft)
                .caret_position(app.model_caret)
                .placeholder("Filter models")
                .change(move |draft| {
                    changed.replace_model_draft(draft.to_string());
                })
                .submit(move |_| submit.commit_model_edit())
                .height(34.0)
                .class(classes!("rounded-md border border-gray-6 bg-gray-1")),
        );
    let choices = app.filtered_models();
    for (index, choice) in choices.iter().enumerate() {
        let pick = app.clone();
        let picked = choice.clone();
        let mut title = picked.split_once('\t').map_or_else(
            || picked.clone(),
            |(endpoint, model)| format!("{endpoint} · {model}"),
        );
        if app.model.borrow().is_model(choice) {
            title.push_str(" · current");
        }
        panel = panel.child(
            hit(title.clone(), app.model_sel.peek() == index, move || {
                let choices = pick.filtered_models();
                if let Some(selected) = choices.iter().position(|choice| choice == &picked) {
                    pick.model_sel.set(selected);
                    pick.commit_model_edit();
                }
            })
            .child(
                label(title)
                    .ellipsize()
                    .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 16.0))
                    .class(classes!("text-sm text-gray-12")),
            ),
        );
    }
    if app.model_loading.peek() {
        panel = panel.child(label("Loading models…").class(classes!("text-xs text-gray-11")));
    } else if choices.is_empty() {
        panel = panel
            .child(label("No matches. Change the filter.").class(classes!("text-xs text-gray-11")));
    }
    let error = app.model_error.peek();
    if !error.is_empty() {
        panel = panel.child(label(error).class(classes!("text-xs text-error-11")));
    }
    panel
}

pub(super) fn workflow(app: App) -> NodeBuilder {
    let back = app.clone();
    let clear = app.clone();
    let save = app.clone();
    let previous = app.clone();
    let next = app.clone();
    let name = app.workflow_name.peek();
    let title = if name.is_empty() {
        "Workflow".to_string()
    } else {
        format!("Workflow · {name}")
    };
    let driving = {
        let model = app.model.borrow();
        format!("{} · {}", model.snapshot.endpoint, model.snapshot.model)
    };
    let seats = app.workflow_seats.peek();
    let mut panel = pane()
        .child(header(title, move || back.close_workflow()))
        .child(label("Choose a role to assign its seat.").class(classes!("text-xs text-gray-11")));
    for (index, role) in WorkflowRole::ALL.into_iter().enumerate() {
        let pick = app.clone();
        let seat = seats
            .get(index)
            .and_then(|seat| seat.clone())
            .unwrap_or_else(|| driving.clone());
        panel = panel.child(
            hit(
                role.name().to_string(),
                app.workflow_sel.peek() == index,
                move || {
                    pick.workflow_sel.set(index);
                    pick.pick_workflow_seat();
                },
            )
            .child(label(role.name()).class(classes!("text-sm text-gray-12")))
            .child(spacer())
            .child(
                label(seat)
                    .ellipsize()
                    .layout(
                        LayoutStyle::block()
                            .grow(1.0)
                            .max_size(190.0, f32::INFINITY)
                            .min_size(0.0, 16.0),
                    )
                    .class(classes!("text-xs text-gray-11")),
            ),
        );
    }
    if app.workflow_naming.peek() {
        let submit = app.clone();
        panel = panel.child(
            text_edit(app.workflow_draft)
                .caret_position(app.workflow_caret)
                .placeholder("Workflow name")
                .submit(move |_| submit.commit_workflow())
                .height(34.0)
                .class(classes!("rounded-md border border-gray-6 bg-gray-1")),
        );
    } else {
        let saved = app.workflow_names.peek();
        panel = panel
            .child(
                label(if saved.is_empty() {
                    "No saved workflows.".to_string()
                } else {
                    format!("Saved: {}", saved.join(" · "))
                })
                .class(classes!("text-xs text-gray-11")),
            )
            .child(
                container()
                    .layout(LayoutStyle::flex_row().fill_width().gap(6.0))
                    .child(
                        hit("Previous workflow".to_string(), false, move || {
                            previous.cycle_workflow(-1);
                        })
                        .child(label("Previous").class(classes!("text-xs text-gray-11"))),
                    )
                    .child(
                        hit("Next workflow".to_string(), false, move || {
                            next.cycle_workflow(1);
                        })
                        .child(label("Next").class(classes!("text-xs text-gray-11"))),
                    ),
            )
            .child(
                container()
                    .layout(LayoutStyle::flex_row().fill_width().gap(6.0))
                    .child(
                        hit("Clear seat".to_string(), false, move || {
                            clear.clear_workflow_seat();
                        })
                        .child(label("Clear seat").class(classes!("text-xs text-gray-11"))),
                    )
                    .child(
                        hit("Save workflow".to_string(), false, move || {
                            save.begin_workflow_save();
                        })
                        .child(label("Save as…").class(classes!("text-xs text-gray-11"))),
                    ),
            );
    }
    panel
}

fn pane() -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .width(WIDTH)
                .fill_height()
                .padding(16.0)
                .gap(6.0)
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .overflow_y(Overflow::Scroll),
        )
        .class(classes!("bg-gray-2 border-l border-gray-6"))
}

fn header(title: impl Into<String>, close: impl Fn() + 'static) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(32.0)
                .align_items(AlignItems::Center),
        )
        .child(label(title.into()).class(classes!("text-sm font-medium text-gray-12")))
        .child(spacer())
        .child(
            hit("Close".to_string(), false, close)
                .child(label("Close").class(classes!("text-xs text-gray-11"))),
        )
}

fn section(title: &'static str) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(8.0)
                .padding_y(6.0),
        )
        .child(label(title).class(classes!("text-xs text-gray-11")))
}
