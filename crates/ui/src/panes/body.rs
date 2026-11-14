use sigil::prelude::AlignItems;
use sigil::prelude::Disclosure;
use sigil::prelude::IntoNode;
use sigil::prelude::ListRowSlots;
use sigil::prelude::MouseButton;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Overflow;
use sigil::prelude::Paint;
use sigil::prelude::Space;
use sigil::prelude::button;
use sigil::prelude::container;
use sigil::prelude::context_menu;
use sigil::prelude::list_row;
use sigil::prelude::virtual_list;

use super::{App, Pane, PaneRow, diff, todos, tree_actions, view};
use crate::canvas;
use crate::text_style::TextStyle;
use sigil::prelude::{Token, Widget};

pub(super) type Extra = fn(&App, &PaneRow) -> NodeBuilder;

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
    let style = TextStyle::xs(Token::Gray11);
    if canvas::is_window() {
        style.note(text.window).into()
    } else {
        style.line(text.terminal).into()
    }
}

pub(super) fn dim(text: String) -> NodeBuilder {
    TextStyle::xs(Token::Gray11).line(text).into()
}

pub(crate) fn pane_body(app: App, pane: Pane, width: f32) -> NodeBuilder {
    let column = sigil::prelude::column()
        .width(width)
        .padding(Space::Lg)
        .gap(Space::Sm)
        .shrink(0.0)
        .min_width(0.0)
        .min_height(0.0)
        .background(sigil::prelude::Token::Gray3);
    let column = if canvas::is_window() {
        column
    } else {
        column.fill_height().overflow_y(Overflow::Scroll)
    };
    match pane {
        Pane::Diff => diff::diff(app, column, cols_in(width)),
        Pane::View => view::view(app, column),
        Pane::Ledger => todos::ledger(&app, column),
        Pane::Chat | Pane::Terminal | Pane::Custom => column,
        Pane::Files | Pane::Worktrees | Pane::Jobs | Pane::Mcp | Pane::Peers => {
            listed(&app, pane, column)
        }
    }
}

fn listed(app: &App, pane: Pane, column: NodeBuilder) -> NodeBuilder {
    let rows = app.pane_rows(pane);
    let shown = rows.len();
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
        Pane::Chat
        | Pane::Ledger
        | Pane::Files
        | Pane::Diff
        | Pane::View
        | Pane::Terminal
        | Pane::Custom => (
            app.workspace()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            app.git_branch().unwrap_or_default(),
            &NO_FILES,
            None,
        ),
    };
    let column = column.child(header(app, title, note));
    listing(app, pane, rows, column, none, (trailing, None))
}

pub(super) fn listing(
    app: &App,
    pane: Pane,
    rows: Vec<PaneRow>,
    mut column: NodeBuilder,
    none: &Empty,
    (trailing, below): (Option<Extra>, Option<Extra>),
) -> NodeBuilder {
    if rows.is_empty() {
        return column.child(empty(none));
    }
    let cursor = app
        .pane_list(pane)
        .cursor
        .get()
        .min(rows.len().saturating_sub(1));
    let tree = App::pane_tree(pane);
    if below.is_none() {
        let list_state = app.pane_list(pane);
        let row_app = app.clone();
        let estimate = if canvas::is_window() {
            28.0
        } else {
            canvas::cell().h
        };
        let viewport_h = f32::from(app.viewport.peek().1).max(1.0) * canvas::cell().h;
        let list = virtual_list(rows, move |index, row: &PaneRow| {
            let item = row_node(&row_app, pane, index, row, cursor, tree, trailing);
            attach_extras(item, row)
        })
        .measured()
        .estimate(estimate)
        .viewport_height(viewport_h)
        .fill()
        .keyboard(false)
        .scroll_bind(list_state.scroll)
        .follow_row(list_state.cursor);
        if canvas::is_window() {
            column = column.fill_height();
        }
        return column.child(list);
    }
    for (index, row) in rows.into_iter().enumerate() {
        column = column.child(row_node(app, pane, index, &row, cursor, tree, trailing));
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

fn row_node(
    app: &App,
    pane: Pane,
    index: usize,
    row: &PaneRow,
    cursor: usize,
    tree: bool,
    trailing: Option<Extra>,
) -> NodeBuilder {
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
    let mut item = list_row(index == cursor, row.name.clone(), slots);
    if let Some(extra) = trailing {
        item = item.child(extra(app, row));
    }
    let item = item.pointer_down(move |event| {
        if event.button == Some(MouseButton::Right) {
            return point.pane_point(pane, index);
        }
        point.pane_click(pane, index);
        event.stop_propagation();
    });
    context_menu(item, move || menu.row_menu(pane, index))
}

fn attach_extras(node: NodeBuilder, row: &PaneRow) -> NodeBuilder {
    if row.note.is_none() && row.detail.is_empty() {
        return node;
    }
    let mut col = sigil::prelude::column()
        .fill_width()
        .gap(Space::Sm)
        .child(node);
    if let Some(note) = row.note.as_ref() {
        col = col.child(dim(note.clone()));
    }
    if !row.detail.is_empty() {
        col = col.child(detail(row.detail.iter().cloned()));
    }
    col
}

pub(super) fn count(items: usize) -> String {
    if items == 0 {
        String::new()
    } else {
        items.to_string()
    }
}

pub(super) fn header(app: &App, title: String, note: String) -> NodeBuilder {
    let row = sigil::prelude::row()
        .fill_width()
        .align_items(AlignItems::Center)
        .gap(Space::Sm)
        .child(
            sigil::prelude::text(title)
                .nowrap()
                .ellipsize()
                .grow(1.0)
                .shrink(1.0)
                .min_width(0.0)
                .min_height(16.0)
                .font_size(14.0, "14 px text that keeps its own line height and weight")
                .font_weight(500)
                .text_color(if app.pane_focus.get() && !canvas::is_window() {
                    sigil::prelude::Token::Accent
                } else {
                    sigil::prelude::Token::Gray12
                }),
        )
        .child(TextStyle::xs(Token::Gray11).line(note).shrink(0.0));
    if canvas::is_window() {
        return row;
    }
    row.child(container().shrink(0.0).child(close(app.clone())))
}

pub(super) fn detail(lines: impl IntoIterator<Item = String>) -> NodeBuilder {
    let mut detail = sigil::prelude::column()
        .fill_width()
        .gap(Space::raw(1.0, "a one pixel row separation"))
        .radius(sigil::prelude::Radius::Md)
        .background(Token::Gray1);
    for line in lines {
        detail = detail.child(
            TextStyle::xs(Token::Gray11)
                .mono()
                .line(line)
                .ellipsize()
                .fill_width()
                .min_width(0.0)
                .min_height(14.0)
                .padding_x(Space::Sm),
        );
    }
    detail
}

pub(super) fn close(app: App) -> impl IntoNode {
    button("✕")
        .ghost()
        .square()
        .activate(move || app.toggle_side())
        .accessible("Close pane")
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a pane is a few hundred pixels wide, far inside u16"
)]
fn cols_in(width: f32) -> u16 {
    (width / crate::canvas::cell().w).max(0.0) as u16
}

fn tree_buttons(app: &App, row: &PaneRow) -> NodeBuilder {
    tree_actions::buttons(app, std::path::Path::new(&row.path))
}
