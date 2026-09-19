use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, WidgetLayoutExt,
    classes, container, label, modal, signal, text_edit, virtual_list,
};

use super::super::super::Cmd;
use super::super::app::App;
use crate::workflow::Role;

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    container().layout(LayoutStyle::block()).child(
        modal(app.workflow_open)
            .title(title(&app))
            .trap(true)
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .gap(6.0)
                            .fill_width()
                            .fill_height(),
                    )
                    .children_reactive(move || {
                        super::super::view::watch(&paint);
                        if paint.workflow_naming.get() {
                            return naming(&paint);
                        }
                        seats(&paint)
                    }),
            ),
    )
}

fn title(app: &App) -> String {
    let name = app.workflow_name.peek();
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
            text_edit(app.workflow_draft)
                .caret_position(app.workflow_caret)
                .placeholder("Workflow name")
                .submit(move |_| save.commit_workflow())
                .height(32.0),
        ),
        container()
            .child(label("enter saves · esc goes back").class(classes!("text-xs text-gray-11"))),
    ]
}

fn seats(app: &App) -> Vec<NodeBuilder> {
    let source = app.clone();
    let dragging = app.drag_from.get();
    let over = app.drag_over.get();
    let selected = app.workflow_sel.get();
    let driving = {
        let model = app.model.borrow();
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
                .workflow_seats
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
            .row_height(24.0)
            .viewport_fill()
            .scroll_into_view(selected),
        ),
    ];
    items.push(saved(app));
    items
}

fn saved(app: &App) -> NodeBuilder {
    let close = app.clone();
    let names = app.workflow_names.get();
    let current = app.workflow_name.get();
    let mut row = container()
        .layout(LayoutStyle::flex_row().fill_width().gap(6.0).padding_x(8.0))
        .child(label("saved:").class(classes!("text-xs text-gray-11")));
    if names.is_empty() {
        row = row.child(label("none").class(classes!("text-xs text-gray-11")));
        return row;
    }
    for name in names {
        let pick = close.clone();
        let chosen = name.clone();
        let active = name == current;
        row = row.child(
            container()
                .role(sigil::prelude::Role::Button)
                .label(name.clone())
                .focusable(true)
                .class(classes!("rounded-md"))
                .class_when(signal(active), "bg-gray-5")
                .pointer_down(move |event| {
                    drop(pick.cmd.send(Cmd::Command(format!("/workflow {chosen}"))));
                    event.stop_propagation();
                })
                .child(
                    label(name)
                        .class(classes!("text-xs text-gray-12"))
                        .class_when(signal(active), "font-bold"),
                ),
        );
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
    container()
        .role(sigil::prelude::Role::Button)
        .label(format!("{role}"))
        .focusable(true)
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(24.0)
                .padding_x(8.0)
                .gap(8.0),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(lit), "bg-gray-5")
        .pointer_down(move |event| {
            press.workflow_sel.set(index);
            press.drag_from.set(Some(index));
            press.drag_over.set(Some(index));
            event.capture_pointer();
            event.stop_propagation();
        })
        .pointer_move(move |event| {
            if hover.drag_from.peek().is_some() {
                hover.drag_over.set(Some(index));
            }
            event.stop_propagation();
        })
        .pointer_up(move |event| {
            let Some(from) = drop.drag_from.peek() else {
                return;
            };
            drop.drag_from.set(None);
            drop.drag_over.set(None);
            if from == index {
                drop.workflow_sel.set(index);
                drop.pick_workflow_seat();
            } else {
                drop.copy_seat(from, index);
            }
            event.stop_propagation();
        })
        .child(label(format!("{marker} {role}")).class(classes!("text-sm text-gray-12")))
        .child(label(seat.to_string()).class(classes!("text-xs text-gray-11")))
}
