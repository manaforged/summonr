use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, PathNode, classes,
    container, label, list_row, section_label, sidebar_width, spacer,
};

use super::{App, Pane};
use super::{icons, open};
use crate::session;

const RAIL: f32 = 252.0;

pub(super) fn rail(app: App) -> NodeBuilder {
    let mut column = container().label("Panes").layout(
        LayoutStyle::flex_column()
            .fill_width()
            .fill_height()
            .padding_x(12.0)
            .padding_y(14.0)
            .gap(2.0),
    );
    for (pane, title) in [
        (Pane::Files, "Files"),
        (Pane::Diff, "Changes"),
        (Pane::Worktrees, "Worktrees"),
        (Pane::Jobs, "Jobs"),
        (Pane::Peers, "Peers"),
        (Pane::Ledger, "Plan"),
    ] {
        let toggle = app.clone();
        column = column.child(
            list_row(open(&app, pane), title, "")
                .child(glyph(pane))
                .pointer_down(move |event| {
                    toggle.toggle_pane(pane);
                    event.stop_propagation();
                }),
        );
    }
    sidebar_width(
        RAIL,
        column
            .child(section("Sessions"))
            .child(sessions(&app))
            .child(spacer())
            .child(seat(&app)),
    )
}

fn glyph(pane: Pane) -> PathNode {
    match pane {
        Pane::Diff => icons::diffs(),
        Pane::Worktrees => icons::branch(),
        Pane::Jobs => icons::terminal(),
        Pane::Peers => icons::peers(),
        Pane::Ledger => icons::clock(),
        _ => icons::folder(),
    }
}

fn section(title: &'static str) -> NodeBuilder {
    section_label(title)
}

fn sessions(app: &App) -> NodeBuilder {
    let here = app.workspace.display().to_string();
    let current = app.model.get().session.clone();
    let mut rows: Vec<session::SessionInfo> = session::list()
        .unwrap_or_default()
        .into_iter()
        .filter(|info| info.cwd.as_deref() == Some(here.as_str()))
        .collect();
    rows.reverse();
    rows.truncate(12);
    let mut list = container().layout(LayoutStyle::flex_column().fill_width().gap(2.0));
    if rows.is_empty() {
        list = list.child(
            label(format!("session {}", session::short(&current)))
                .class(classes!("text-xs text-gray-11")),
        );
    }
    for info in rows {
        let title = info
            .name
            .clone()
            .unwrap_or_else(|| info.preview.chars().take(36).collect());
        let live = info.id == current;
        let open = app.clone();
        let id = info.id.clone();
        list = list.child(
            list_row(
                live,
                if title.is_empty() {
                    "untitled".to_string()
                } else {
                    title.clone()
                },
                session::short(&info.id).to_string(),
            )
            .pointer_down(move |event| {
                if !live {
                    open.request_resume(id.clone());
                }
                event.stop_propagation();
            }),
        );
    }
    list
}

fn seat(app: &App) -> NodeBuilder {
    let model = app.model.get().snapshot.model.clone();
    list_row(false, app.info.endpoint.clone(), model).child(icons::avatar())
}
