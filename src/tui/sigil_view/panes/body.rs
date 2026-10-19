use sigil::kernel::ObjectFit;
use sigil::layout::Dimension;
use sigil::layout::Overflow;
use sigil::prelude::AlignItems;
use sigil::prelude::IntoNode;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::MouseButton;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::Paint;
use sigil::prelude::Tone;
use sigil::prelude::WidgetLayoutExt;
use sigil::prelude::button;
use sigil::prelude::chip;
use sigil::prelude::container;
use sigil::prelude::icon_button;
use sigil::prelude::image;
use sigil::prelude::list_row;
use sigil::prelude::node;
use sigil::prelude::text_edit;

use super::{App, FileRow, Pane, diff, jobs, mcp, peers, todos};
use crate::tui::sigil_view::canvas;

pub(super) struct Empty {
    pub(super) window: &'static str,
    pub(super) terminal: &'static str,
}

pub(super) fn empty(text: &Empty) -> NodeBuilder {
    let line = if canvas::is_window() {
        sigil::kernel::text(text.window)
            .wrap()
            .layout(LayoutStyle::block().fill_width().min_size(0.0, 14.0))
    } else {
        sigil::kernel::text(text.terminal).nowrap()
    };
    line.font_size(12.0)
        .text_color(sigil::prelude::Token::Gray11)
        .into()
}

const NO_TREES: Empty = Empty {
    window: "Worktrees show here when a session works on its own branch in a separate checkout.",
    terminal: "no worktrees; summonr --worktree, --goal, or --steps",
};

pub(in crate::tui::sigil_view) fn pane_body(app: App, pane: Pane, width: f32) -> NodeBuilder {
    drop((
        app.diff_view.get(),
        app.diff_cursor.get(),
        app.hunk_cursor.get(),
        app.note_target.get(),
        app.pane_focus.get(),
    ));
    let style = LayoutStyle::flex_column()
        .width(width)
        .padding(16.0)
        .gap(8.0)
        .shrink(0.0)
        .min_size(0.0, 0.0);
    let style = if canvas::is_window() {
        style
    } else {
        style.fill_height().overflow_y(Overflow::Scroll)
    };
    let mut column = container().layout(style).bg(sigil::prelude::Token::Gray3);
    column = match pane {
        Pane::Diff => diff::diff(app, column, cols_in(width)),
        Pane::Files => files(app, column),
        Pane::Worktrees => trees(app, column),
        Pane::Jobs => jobs::jobs(app, column),
        Pane::Mcp => mcp::mcp(app, column),
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
        .child(
            sigil::kernel::text(title)
                .nowrap()
                .ellipsize()
                .layout(
                    LayoutStyle::block()
                        .grow(1.0)
                        .shrink(1.0)
                        .min_size(0.0, 16.0),
                )
                .font_size(14.0)
                .font_weight(500)
                .text_color(if app.pane_focus.peek() && !canvas::is_window() {
                    sigil::prelude::Token::Accent
                } else {
                    sigil::prelude::Token::Gray12
                }),
        )
        .child(
            sigil::kernel::text(note)
                .nowrap()
                .layout(LayoutStyle::block().shrink(0.0))
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        )
        .child(
            container()
                .layout(LayoutStyle::block().shrink(0.0))
                .child(close(app.clone())),
        )
}

pub(super) fn detail(lines: impl IntoIterator<Item = String>) -> NodeBuilder {
    let mut detail = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(1.0))
        .rounded(sigil::prelude::Radius::Md)
        .bg(sigil::prelude::Token::Gray1);
    for line in lines {
        detail = detail.child(
            sigil::kernel::text(line)
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .ellipsize()
                .layout(
                    LayoutStyle::block()
                        .fill_width()
                        .min_size(0.0, 14.0)
                        .padding_x(8.0),
                )
                .font_size(12.0)
                .font_family(sigil::prelude::FontFamily::Mono)
                .text_color(sigil::prelude::Token::Gray11),
        );
    }
    detail
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
            sigil::kernel::text("no files in this folder yet")
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
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
        return column.child(empty(&NO_TREES));
    }
    for (index, row) in trees.into_iter().enumerate() {
        let show = app.clone();
        let path = row.tree.path.clone();
        let branch = row.tree.branch.clone();
        let menu = app.clone();
        column = column.child(
            list_row(
                false,
                row.tree.branch.clone(),
                format!("{} ahead", row.ahead),
            )
            .child(chip(
                if row.dirty { "dirty" } else { "clean" },
                if row.dirty { Tone::Warn } else { Tone::Neutral },
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
        return column.child(
            sigil::kernel::text("no file open")
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        );
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
                    sigil::kernel::text(view.path.clone())
                        .nowrap()
                        .text_color(sigil::prelude::Token::OnSurface)
                        .font_size(14.0)
                        .font_weight(500)
                        .text_color(sigil::prelude::Token::Gray12),
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
        return column.child(node(move || {
            text_edit(app.view_text)
                .caret_position(app.view_caret)
                .autofocus()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .height(Dimension::Auto)
                        .grow(1.0)
                        .min_size(0.0, 0.0),
                )
                .font_family(sigil::prelude::FontFamily::Mono)
                .font_size(12.0)
                .bg(sigil::prelude::Token::Gray1)
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
        .rounded(sigil::prelude::Radius::Md)
        .bg(sigil::prelude::Token::Gray1);
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
            row = row.bg(sigil::prelude::Token::Success3);
        }
        body = body.child(
            row.child(
                sigil::kernel::text(number.to_string())
                    .nowrap()
                    .text_color(sigil::prelude::Token::OnSurface)
                    .font_size(12.0)
                    .text_color(sigil::prelude::Token::Gray11),
            )
            .child(
                sigil::kernel::text(line.replace('\t', "    "))
                    .nowrap()
                    .text_color(sigil::prelude::Token::OnSurface)
                    .ellipsize()
                    .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                    .font_size(12.0)
                    .font_family(sigil::prelude::FontFamily::Mono)
                    .text_color(sigil::prelude::Token::Gray12),
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

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a pane is a few hundred pixels wide, far inside u16"
)]
fn cols_in(width: f32) -> u16 {
    (width / crate::tui::sigil_view::canvas::cell().w).max(0.0) as u16
}
