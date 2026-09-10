use sigil::prelude::*;

use super::super::app::{App, FileRow};
use super::super::pane::Pane;
use super::{hit, options, spacer};

const WIDTH: f32 = 360.0;

pub(super) fn slot(app: App) -> NodeBuilder {
    let mut slot = container().layout(LayoutStyle::flex_row().fill_height().shrink(0.0));
    if app.editing_model.peek() {
        slot = slot.child(options::models(app));
    } else if app.workflow_open.peek() {
        slot = slot.child(options::workflow(app));
    } else if app.hotkeys_open.peek() {
        slot = slot.child(options::hotkeys(app));
    } else if app.settings_open.peek() {
        slot = slot.child(options::main(app));
    } else if let Some(pane) = app.layout.peek().secondary {
        slot = slot.child(pane_body(app, pane));
    }
    slot
}

fn pane_body(app: App, pane: Pane) -> NodeBuilder {
    let mut column = container()
        .layout(
            LayoutStyle::flex_column()
                .width(WIDTH)
                .fill_height()
                .padding(16.0)
                .gap(8.0)
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .overflow_y(Overflow::Scroll),
        )
        .class(classes!("bg-gray-2 border-l border-gray-6"));
    column = match pane {
        Pane::Diff => diff(app, column),
        Pane::Files => files(app, column),
        Pane::Worktrees => trees(app, column),
        Pane::View => view(app, column),
        Pane::Ledger => ledger(&app, column),
        Pane::Chat => column,
    };
    column
}

fn header(app: &App, title: String, note: String) -> NodeBuilder {
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
        .child(close(app))
}

fn close(app: &App) -> NodeBuilder {
    let close = app.clone();
    hit("Close pane".to_string(), false, move || close.toggle_side())
        .layout(
            LayoutStyle::flex_row()
                .fixed_size(28.0, 28.0)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center),
        )
        .child(label("×").class(classes!("text-xs text-gray-11")))
}

fn diff(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.borrow();
    if let Some(path) = app.diff_view.peek() {
        let back = app.clone();
        column = column
            .child(header(
                &app,
                path.clone(),
                format!("vs {}", model.diff.base),
            ))
            .child(
                hit("Back to changes".to_string(), false, move || {
                    back.pane_back();
                })
                .child(label("← all changes").class(classes!("text-xs text-gray-11"))),
            );
        let Some((_, text)) = model.patch.as_ref().filter(|(seen, _)| *seen == path) else {
            return column.child(label("loading…").class(classes!("text-xs text-gray-11")));
        };
        let mut hunk = container()
            .layout(LayoutStyle::flex_column().fill_width().gap(1.0))
            .class(classes!("rounded-md bg-gray-1"));
        for line in text.lines() {
            let (class, tint) = if line.starts_with("+++") || line.starts_with("---") {
                ("text-xs font-mono text-gray-11", "")
            } else if line.starts_with('+') {
                ("text-xs font-mono text-success-11", "bg-success-3")
            } else if line.starts_with('-') {
                ("text-xs font-mono text-error-11", "bg-error-3")
            } else if line.starts_with("@@") {
                ("text-xs font-mono text-info-11", "")
            } else {
                ("text-xs font-mono text-gray-12", "")
            };
            let mut row = container().layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .padding_x(8.0)
                    .padding_y(1.0),
            );
            if !tint.is_empty() {
                row = row.class(tint);
            }
            hunk = hunk.child(
                row.child(
                    label(line.replace('\t', "    "))
                        .ellipsize()
                        .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                        .class(class),
                ),
            );
        }
        return column.child(hunk);
    }
    column = column.child(header(&app, "Changes".to_string(), model.diff.summary()));
    drop(model);
    let stats = app.diff_stats();
    if stats.is_empty() {
        return column.child(label("no changes").class(classes!("text-xs text-gray-11")));
    }
    for stat in stats {
        let open = app.clone();
        let path = stat.path.clone();
        column = column.child(
            hit(stat.path.clone(), false, move || {
                open.diff_open(path.clone())
            })
            .child(
                label(stat.path.clone())
                    .ellipsize()
                    .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                    .class(classes!("text-xs text-gray-12")),
            )
            .child(label(format!("+{}", stat.added)).class(classes!("text-xs text-success-11")))
            .child(label(format!("-{}", stat.removed)).class(classes!("text-xs text-error-11"))),
        );
    }
    column
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
        return column.child(label("no files listed yet").class(classes!("text-xs text-gray-11")));
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
        column = column.child(
            hit(row.path.clone(), false, move || {
                if dir {
                    act.file_cursor.set(index);
                    act.pane_enter();
                } else {
                    act.view_open(path.clone());
                }
            })
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .height(24.0)
                    .padding_x(8.0 + 12.0 * row.depth as f32)
                    .gap(6.0)
                    .align_items(AlignItems::Center),
            )
            .child(label(glyph).class(classes!("text-xs text-gray-11")))
            .child(label(row.name).class(if row.dir {
                classes!("text-xs text-gray-12")
            } else {
                classes!("text-xs text-gray-11")
            })),
        );
    }
    column
}

fn trees(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let trees = app.model.borrow().trees.clone();
    column = column.child(header(
        &app,
        "Worktrees".to_string(),
        trees.len().to_string(),
    ));
    if trees.is_empty() {
        return column.child(
            label("no worktrees — summon --worktree, --goal, or --steps")
                .class(classes!("text-xs text-gray-11")),
        );
    }
    for row in trees {
        let show = app.clone();
        let path = row.tree.path.clone();
        let branch = row.tree.branch.clone();
        column = column.child(
            hit(row.tree.branch.clone(), false, move || {
                show.tree_show(path.clone(), branch.clone());
            })
            .child(
                label(row.tree.branch.clone())
                    .ellipsize()
                    .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                    .class(classes!("text-xs text-gray-12")),
            )
            .child(label(format!("+{}", row.ahead)).class(classes!("text-xs text-gray-11")))
            .child(badge(
                if row.dirty { "dirty" } else { "clean" },
                if row.dirty {
                    Tone::Error
                } else {
                    Tone::Neutral
                },
            )),
        );
    }
    column
}

fn view(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.borrow();
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
            label(view.path.clone())
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                .class(classes!("text-sm font-medium text-gray-12")),
        );
    if editing {
        bar = bar
            .child(
                hit("Save".to_string(), false, move || save.edit_save())
                    .child(label("Save").class(classes!("text-xs text-gray-12"))),
            )
            .child(
                hit("Leave".to_string(), false, move || {
                    leave.edit_stop();
                })
                .child(label("Leave").class(classes!("text-xs text-gray-11"))),
            );
    } else {
        bar = bar.child(
            hit("Edit".to_string(), false, move || edit.edit_start())
                .child(label("Edit").class(classes!("text-xs text-gray-12"))),
        );
    }
    bar = bar.child(close(&app));
    column = column.child(bar);
    if editing {
        return column.child(
            text_edit(app.view_text)
                .caret_position(app.view_caret)
                .layout(
                    LayoutStyle::block()
                        .fill_width()
                        .grow(1.0)
                        .min_size(0.0, 200.0),
                )
                .class(classes!("font-mono text-xs bg-gray-1")),
        );
    }
    let text = view.rope.to_string_lossy();
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
    column.child(body)
}

fn ledger(app: &App, mut column: NodeBuilder) -> NodeBuilder {
    let lines = app.model.borrow().ledger.clone();
    column = column.child(header(app, "Plan".to_string(), String::new()));
    if lines.is_empty() {
        return column.child(
            label("no plan is running — /steps PLAN.md or --goal")
                .class(classes!("text-xs text-gray-11")),
        );
    }
    for line in lines {
        let done = line.contains("[x]");
        column = column.child(text(line).class(if done {
            classes!("text-xs text-gray-11")
        } else {
            classes!("text-xs text-gray-12")
        }));
    }
    column
}
