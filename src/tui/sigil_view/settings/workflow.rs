use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, WidgetLayoutExt,
    button, classes, container, label, text_edit, virtual_list,
};

use super::prelude::*;
use super::{App, canvas};
use crate::tui::Cmd;
use crate::workflow::Role;

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    vec![
        press("d", |app, event| {
            if app.workflow_keys() {
                event.stop_propagation();
                app.clear_workflow_seat();
            }
        }),
        press("s", |app, event| {
            if app.workflow_keys() {
                event.stop_propagation();
                app.begin_workflow_save();
            }
        }),
    ]
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    shell(
        app.workflow.open,
        title(&app),
        app.viewport.peek().0,
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .children_reactive(move || {
                if paint.workflow.naming.get() {
                    return naming(&paint);
                }
                seats(&paint)
            }),
    )
}

fn title(app: &App) -> String {
    let name = app.workflow.name.peek();
    if name.is_empty() {
        "⚙ workflow (unsaved)".to_string()
    } else {
        format!("⚙ workflow {name}")
    }
}

fn naming(app: &App) -> Vec<NodeBuilder> {
    let save = app.clone();
    vec![
        container().child(
            text_edit(app.workflow.draft)
                .caret_position(app.workflow.caret)
                .placeholder("Workflow name")
                .submit(move |_| save.commit_workflow())
                .height(canvas::overlay_field()),
        ),
        container()
            .child(label("enter saves · esc goes back").class(classes!("text-xs text-gray-11"))),
    ]
}

fn seats(app: &App) -> Vec<NodeBuilder> {
    let source = app.clone();
    let dragging = app.workflow.drag_from.get();
    let over = app.workflow.drag_over.get();
    let selected = app.workflow.sel.get();
    let driving = {
        let model = app.model.get();
        format!(
            "driving · {} {}",
            model.snapshot.endpoint, model.snapshot.model
        )
    };
    let lookup: Vec<(Role, String)> = Role::ALL
        .iter()
        .enumerate()
        .map(|(index, role)| {
            let seat = app
                .workflow
                .seats
                .get()
                .get(index)
                .cloned()
                .flatten()
                .map(|seat| seat.replace('\t', "  "))
                .unwrap_or_else(|| driving.clone());
            (*role, seat)
        })
        .collect();
    let mut items: Vec<NodeBuilder> = vec![
        container().layout(LayoutStyle::block().fill_width()).child(
            virtual_list(lookup, move |index, (role, seat): &(Role, String)| {
                entry(
                    source.clone(),
                    *role,
                    seat,
                    index,
                    index == selected,
                    dragging == Some(index),
                    over == Some(index),
                )
            })
            .row_height(canvas::overlay_list_row())
            .height(canvas::overlay_list_row() * 8.0)
            .viewport_height(canvas::overlay_list_row() * 8.0)
            .scroll_into_view(selected),
        ),
    ];
    items.push(saved(app));
    items
}

fn saved(app: &App) -> NodeBuilder {
    let close = app.clone();
    let names = app.workflow.names.get();
    let current = app.workflow.name.get();
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(canvas::overlay_gap())
                .padding_x(8.0),
        )
        .child(label("saved:").class(classes!("text-xs text-gray-11")));
    if names.is_empty() {
        row = row.child(label("none").class(classes!("text-xs text-gray-11")));
        return row;
    }
    for name in names {
        let pick = close.clone();
        let chosen = name.clone();
        let active = name == current;
        let chip = if active {
            button(name).primary()
        } else {
            button(name).ghost()
        };
        row = row.child(chip.activate(move || {
            drop(pick.cmd.send(Cmd::Command(format!("/workflow {chosen}"))));
        }));
    }
    row
}

fn entry(
    app: App,
    role: Role,
    seat: &str,
    index: usize,
    active: bool,
    dragging: bool,
    over: bool,
) -> NodeBuilder {
    let press = app.clone();
    let hover = app.clone();
    let drop = app.clone();
    let marker = if dragging {
        '◆'
    } else if over {
        '▸'
    } else if active {
        '›'
    } else {
        ' '
    };
    let lit = active || over;
    pick(lit, format!("{marker} {role}"), seat.to_string())
        .pointer_down(move |event| {
            press.workflow.sel.set(index);
            press.workflow.drag_from.set(Some(index));
            press.workflow.drag_over.set(Some(index));
            event.capture_pointer();
            event.stop_propagation();
        })
        .pointer_move(move |event| {
            if hover.workflow.drag_from.peek().is_some() {
                hover.workflow.drag_over.set(Some(index));
            }
            event.stop_propagation();
        })
        .pointer_up(move |event| {
            let Some(from) = drop.workflow.drag_from.peek() else {
                return;
            };
            drop.workflow.drag_from.set(None);
            drop.workflow.drag_over.set(None);
            if from == index {
                drop.workflow.sel.set(index);
                drop.pick_workflow_seat();
            } else {
                drop.copy_seat(from, index);
            }
            event.stop_propagation();
        })
}
