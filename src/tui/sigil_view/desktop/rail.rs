//! Left rail: pane switches, this workspace's sessions, the seat.

use sigil::prelude::*;

use super::super::app::App;
use super::super::pane::Pane;
use super::{hit, icons, open, spacer};
use crate::session;

const RAIL: f32 = 252.0;

pub(super) fn rail(app: App) -> NodeBuilder {
    let mut column = container()
        .role(Role::Navigation)
        .label("Panes")
        .layout(
            LayoutStyle::flex_column()
                .width(RAIL)
                .fill_height()
                .padding_x(12.0)
                .padding_y(14.0)
                .gap(2.0)
                .shrink(0.0),
        )
        .class(classes!("bg-gray-3"));
    for (pane, title) in [
        (Pane::Files, "Files"),
        (Pane::Diff, "Changes"),
        (Pane::Worktrees, "Worktrees"),
        (Pane::Ledger, "Plan"),
    ] {
        let toggle = app.clone();
        column = column.child(
            hit(title.to_string(), open(&app, pane), move || {
                toggle.toggle_pane(pane);
            })
            .child(glyph(pane))
            .child(label(title).class(classes!("text-sm text-gray-12"))),
        );
    }
    column
        .child(section("Sessions"))
        .child(sessions(&app))
        .child(spacer())
        .child(seat(&app))
}

fn glyph(pane: Pane) -> PathNode {
    match pane {
        Pane::Diff => icons::diffs(),
        Pane::Worktrees => icons::branch(),
        Pane::Ledger => icons::clock(),
        _ => icons::folder(),
    }
}

fn section(title: &'static str) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_y(10.0)
                .padding_x(8.0)
                .align_items(AlignItems::Center),
        )
        .child(label(title).class(classes!("text-xs text-gray-11")))
}

/// Sessions that ran in this workspace, newest first; the live one is lit.
fn sessions(app: &App) -> NodeBuilder {
    let here = app.workspace.display().to_string();
    let current = app.info.session.clone();
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
            hit(title.clone(), live, move || {
                if !live {
                    open.request_resume(id.clone());
                }
            })
            .child(status_dot(live))
            .child(
                label(if title.is_empty() {
                    "untitled".to_string()
                } else {
                    title
                })
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 16.0))
                .class(classes!("text-sm text-gray-12")),
            )
            .child(
                label(session::short(&info.id).to_string()).class(classes!("text-xs text-gray-11")),
            ),
        );
    }
    list
}

fn status_dot(live: bool) -> NodeBuilder {
    let dot = container().layout(LayoutStyle::block().fixed_size(8.0, 8.0));
    if live {
        dot.class(classes!("rounded-full bg-success-9"))
    } else {
        dot.class(classes!("rounded-full border border-gray-8"))
    }
}

fn seat(app: &App) -> NodeBuilder {
    let model = app.model.borrow().model.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(36.0)
                .padding_x(8.0)
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .child(icons::avatar())
        .child(
            label(format!("{} · {model}", app.info.endpoint))
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 16.0))
                .class(classes!("text-sm text-gray-12")),
        )
}
