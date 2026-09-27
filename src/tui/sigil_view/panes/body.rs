use sigil::prelude::{
    AlignItems, Cx, IntoNode, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder,
    NodeEventsExt, NodeStyleExt, ObjectFit, Overflow, Tone, WidgetLayoutExt, badge, button,
    classes, container, icon_button, image, label, list_row, node, request_focus, spacer,
    text_edit,
};

use super::{App, FileRow, Pane, diff, jobs, peers, todos};

pub(in crate::tui::sigil_view) fn pane_body(app: App, pane: Pane, width: f32) -> NodeBuilder {
    let mut column = container()
        .layout(
            LayoutStyle::flex_column()
                .width(width)
                .fill_height()
                .padding(16.0)
                .gap(8.0)
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .overflow_y(Overflow::Scroll),
        )
        .class(classes!("bg-gray-2 border-l border-gray-6"));
    column = match pane {
        Pane::Diff => diff::diff(app, column),
        Pane::Files => files(app, column),
        Pane::Worktrees => trees(app, column),
        Pane::Jobs => jobs::jobs(app, column),
        Pane::Peers => peers::peers(app, column),
        Pane::View => view(app, column),
        Pane::Ledger => todos::ledger(&app, column),
        Pane::Chat => column,
    };
    column
}

pub(super) fn header(app: &App, title: String, note: String) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .child(label(title).class(classes!("text-sm font-medium text-gray-12")))
        .child(spacer())
        .child(label(note).class(classes!("text-xs text-gray-11")))
        .child(close(app.clone()))
}

fn close(app: App) -> impl IntoNode {
    icon_button("✕", move || app.toggle_side()).accessible("Close pane")
}

fn files(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let repo = app
        .workspace
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    column = column.child(header(
        &app,
        repo,
        app.git_branch.clone().unwrap_or_default(),
    ));
    let rows: Vec<FileRow> = app.file_rows();
    if rows.is_empty() {
        return column.child(
            label("No files in this folder yet.".to_string())
                .class(classes!("text-sm text-gray-11")),
        );
    }
    for (index, row) in rows.into_iter().enumerate() {
        let act = app.clone();
        let dir = row.dir;
        let path = row.path.clone();
        let glyph = if row.dir {
            if row.open { "▾" } else { "▸" }
        } else {
            " "
        };
        let menu = app.clone();
        let title = format!("{glyph} {}", row.name);
        column = column.child(
            list_row(false, title, String::new())
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .height(24.0)
                        .padding_x(8.0 + 12.0 * row.depth as f32)
                        .gap(6.0)
                        .align_items(AlignItems::Center),
                )
                .pointer_down(move |event| {
                    if event.button == Some(MouseButton::Right) {
                        menu.context_row(index);
                    } else if dir {
                        act.file_cursor.set(index);
                        act.pane_enter();
                    } else {
                        act.view_open(path.clone());
                    }
                    event.stop_propagation();
                }),
        );
    }
    column
}

fn trees(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let trees = app.model.get().trees.clone();
    column = column.child(header(
        &app,
        "Worktrees".to_string(),
        trees.len().to_string(),
    ));
    if trees.is_empty() {
        return column.child(
            label("no worktrees — summonr --worktree, --goal, or --steps")
                .class(classes!("text-xs text-gray-11")),
        );
    }
    for (index, row) in trees.into_iter().enumerate() {
        let show = app.clone();
        let path = row.tree.path.clone();
        let branch = row.tree.branch.clone();
        let menu = app.clone();
        column = column.child(
            list_row(false, row.tree.branch.clone(), format!("+{}", row.ahead))
                .child(badge(
                    if row.dirty { "dirty" } else { "clean" },
                    if row.dirty {
                        Tone::Error
                    } else {
                        Tone::Neutral
                    },
                ))
                .pointer_down(move |event| {
                    if event.button == Some(MouseButton::Right) {
                        menu.context_row(index);
                    } else {
                        show.tree_show(path.clone(), branch.clone());
                    }
                    event.stop_propagation();
                }),
        );
    }
    column
}

fn view(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let Some(view) = model.view.as_ref() else {
        return column.child(label("no file open").class(classes!("text-xs text-gray-11")));
    };
    let editing = app.view_edit.peek();
    let edit = app.clone();
    let save = app.clone();
    let leave = app.clone();
    let mut bar = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::block()
                        .grow(1.0)
                        .min_size(0.0, 14.0)
                        .overflow_x(Overflow::Hidden),
                )
                .child(
                    label(view.path.clone()).class(classes!("text-sm font-medium text-gray-12")),
                ),
        );
    if editing {
        bar = bar
            .child(button("Save").primary().activate(move || save.edit_save()))
            .child(button("Leave").ghost().activate(move || {
                leave.edit_stop();
            }));
    } else {
        bar = bar.child(button("Edit").ghost().activate(move || edit.edit_start()));
    }
    bar = bar.child(close(app.clone()));
    column = column.child(bar);
    if editing {
        return column.child(node(move |cx: &mut Cx| {
            let id = cx.add(
                text_edit(app.view_text)
                    .caret_position(app.view_caret)
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .grow(1.0)
                            .min_size(0.0, 0.0),
                    )
                    .class(classes!("font-mono text-xs bg-gray-1")),
            );
            request_focus(id);
            id
        }));
    }
    let text = &view.text;
    let added = |number: usize| {
        view.added
            .iter()
            .any(|(start, count)| number >= *start && number < start + count)
    };
    let mut body = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(1.0))
        .class(classes!("rounded-md bg-gray-1"));
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let mut row = container().layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(8.0)
                .padding_y(1.0)
                .gap(10.0),
        );
        if added(number) {
            row = row.class(classes!("bg-success-3"));
        }
        body = body.child(
            row.child(label(number.to_string()).class(classes!("text-xs text-gray-11")))
                .child(
                    label(line.replace('\t', "    "))
                        .ellipsize()
                        .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                        .class(classes!("text-xs font-mono text-gray-12")),
                ),
        );
    }
    column = column.child(body);
    if let Some(source) = view.image.clone() {
        let height = (source.height() as f32).min(320.0);
        column = column.child(
            image(source)
                .fit(ObjectFit::Contain)
                .layout(LayoutStyle::block().fill_width().min_size(0.0, height)),
        );
    }
    column
}
