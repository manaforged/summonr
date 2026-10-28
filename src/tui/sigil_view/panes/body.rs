use sigil::layout::Overflow;
use sigil::prelude::AlignItems;
use sigil::prelude::Disclosure;
use sigil::prelude::IntoNode;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::ListRowSlots;
use sigil::prelude::MouseButton;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::Paint;
use sigil::prelude::container;
use sigil::prelude::context_menu;
use sigil::prelude::icon_button;
use sigil::prelude::list_row_with;

use super::{App, FileRow, Pane, diff, todos, tree_actions, view};
use crate::tui::sigil_view::canvas;

pub(super) type Extra = fn(&App, &FileRow) -> NodeBuilder;

pub(super) struct Empty {
    pub(super) window: &'static str,
    pub(super) terminal: &'static str,
}

const NO_FILES: Empty = Empty {
    window: "no files in this folder yet",
    terminal: "no files in this folder yet",
};
const NO_TREES: Empty = Empty {
    window: "Worktrees show here when a session works on its own branch in a separate checkout.",
    terminal: "no worktrees; summonr --worktree, --goal, or --steps",
};
const NO_JOBS: Empty = Empty {
    window: "Commands the agent leaves running in the background show here, with their output and exit status.",
    terminal: "no background commands; run_command background:true starts one",
};
const NO_MCP: Empty = Empty {
    window: "no mcp servers; summonr mcp add configures one",
    terminal: "no mcp servers; summonr mcp add configures one",
};
const NO_PEERS: Empty = Empty {
    window: "Other summonr sessions running on this computer show here. Pick one to mention it in your prompt.",
    terminal: "no other live sessions",
};

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

pub(super) fn dim(text: String) -> NodeBuilder {
    sigil::kernel::text(text)
        .nowrap()
        .font_size(12.0)
        .text_color(sigil::prelude::Token::Gray11)
        .into()
}

pub(in crate::tui::sigil_view) fn pane_body(app: App, pane: Pane, width: f32) -> NodeBuilder {
    drop((
        app.diff_view.get(),
        app.pane_list(pane).cursor.get(),
        app.hunk_list.cursor.get(),
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
    let column = container().layout(style).bg(sigil::prelude::Token::Gray3);
    match pane {
        Pane::Diff => diff::diff(app, column, cols_in(width)),
        Pane::View => view::view(app, column),
        Pane::Ledger => todos::ledger(&app, column),
        Pane::Chat => column,
        Pane::Files | Pane::Worktrees | Pane::Jobs | Pane::Mcp | Pane::Peers => {
            listed(&app, pane, column)
        }
    }
}

fn listed(app: &App, pane: Pane, column: NodeBuilder) -> NodeBuilder {
    let shown = app.pane_rows(pane).len();
    let (title, note, none, trailing): (String, String, &Empty, Option<Extra>) = match pane {
        Pane::Worktrees => (
            "Worktrees".to_string(),
            count(shown),
            &NO_TREES,
            Some(tree_buttons as Extra),
        ),
        Pane::Jobs => ("Jobs".to_string(), count(shown), &NO_JOBS, None),
        Pane::Mcp => ("Servers".to_string(), count(shown), &NO_MCP, None),
        Pane::Peers => ("Peers".to_string(), count(shown), &NO_PEERS, None),
        Pane::Chat | Pane::Ledger | Pane::Files | Pane::Diff | Pane::View => (
            app.workspace
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            app.git_branch.clone().unwrap_or_default(),
            &NO_FILES,
            None,
        ),
    };
    let column = column.child(header(app, title, note));
    listing(app, pane, column, none, (trailing, None))
}

pub(super) fn listing(
    app: &App,
    pane: Pane,
    mut column: NodeBuilder,
    none: &Empty,
    (trailing, below): (Option<Extra>, Option<Extra>),
) -> NodeBuilder {
    let rows = app.pane_rows(pane);
    if rows.is_empty() {
        return column.child(empty(none));
    }
    let cursor = app.pane_list(pane).at(rows.len());
    let tree = App::pane_tree(pane);
    for (index, row) in rows.into_iter().enumerate() {
        let (menu, point) = (app.clone(), app.clone());
        let disclosure = match (row.dir, row.open) {
            (true, true) => Disclosure::Expanded,
            (true, false) => Disclosure::Collapsed,
            (false, _) if tree => Disclosure::Leaf,
            (false, _) => Disclosure::None,
        };
        let slots = ListRowSlots {
            depth: u16::try_from(row.depth).unwrap_or(u16::MAX),
            disclosure,
            status: (!row.status.is_empty()).then(|| row.status.clone().into()),
            badge: row.badge.clone().map(|(label, tone)| (label.into(), tone)),
            ..ListRowSlots::default()
        };
        let mut item = list_row_with(index == cursor, row.name.clone(), slots);
        if let Some(extra) = trailing {
            item = item.child(extra(app, &row));
        }
        let item = item.pointer_down(move |event| {
            if event.button == Some(MouseButton::Right) {
                return point.pane_point(pane, index);
            }
            point.pane_click(pane, index);
            event.stop_propagation();
        });
        column = column.child(context_menu(item, move || menu.row_menu(pane, index)));
        if let Some(note) = row.note.clone() {
            column = column.child(dim(note));
        }
        if !row.detail.is_empty() {
            column = column.child(detail(row.detail.clone()));
        }
        if let Some(extra) = below {
            column = column.child(extra(app, &row));
        }
    }
    column
}

pub(super) fn count(items: usize) -> String {
    if items == 0 {
        String::new()
    } else {
        items.to_string()
    }
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

pub(super) fn close(app: App) -> impl IntoNode {
    icon_button("✕", move || app.toggle_side()).accessible("Close pane")
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a pane is a few hundred pixels wide, far inside u16"
)]
fn cols_in(width: f32) -> u16 {
    (width / crate::tui::sigil_view::canvas::cell().w).max(0.0) as u16
}

fn tree_buttons(app: &App, row: &FileRow) -> NodeBuilder {
    tree_actions::buttons(app, std::path::Path::new(&row.path))
}
