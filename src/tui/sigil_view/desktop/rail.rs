use sigil::kernel::TextContent;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, PathNode, Tone, WidgetLayoutExt,
    container, dot, list_row, section_label, sidebar, spacer,
};

use super::{App, Pane};
use super::{icons, open};
use crate::session;
use crate::tui::hub::{Live, Open, Status};

const RAIL: f32 = 252.0;
const PREVIEW_CELLS: usize = 36;
const RECENT: usize = 8;
const SECTION_GAP: f32 = 12.0;
pub(super) const NEW_SESSION: &str = "New session";
pub(super) const UNTITLED: &str = "Untitled";
pub(super) const LEDGER: &str = "Ledger";

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
        (Pane::Ledger, LEDGER),
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
    container()
        .layout(LayoutStyle::flex_column().fill_height().shrink(0.0))
        .child(
            sidebar(
                column
                    .child(section("Sessions"))
                    .child(sessions(&app))
                    .child(spacer())
                    .child(seat(&app)),
            )
            .width(RAIL),
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

fn section(title: impl Into<TextContent>) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_top(SECTION_GAP),
        )
        .child(section_label(title))
}

fn sessions(app: &App) -> NodeBuilder {
    let live = {
        let model = app.model;
        sigil::reactive::derived(move || model.get().live.clone())
    };
    let here = app.workspace.display().to_string();
    let mut recent: Vec<session::SessionInfo> = session::list()
        .unwrap_or_default()
        .into_iter()
        .filter(|info| info.cwd.as_deref() == Some(here.as_str()) && info.depth == 0)
        .collect();
    recent.reverse();
    let open = app.clone();
    container()
        .layout(LayoutStyle::flex_column().fill_width().gap(2.0))
        .children_reactive(move || {
            let live = live.get();
            let mut rows = vec![new_session(&open)];
            for project in projects(&live) {
                rows.push(section(project.clone()));
                rows.extend(
                    live.iter()
                        .filter(|entry| project_of(entry) == project)
                        .map(|entry| live_row(&open, entry)),
                );
            }
            let past: Vec<NodeBuilder> = recent
                .iter()
                .filter(|info| live.iter().all(|entry| entry.session != info.id))
                .take(RECENT)
                .map(|info| past_row(&open, info))
                .collect();
            if !past.is_empty() {
                rows.push(section("Recent"));
                rows.extend(past);
            }
            rows
        })
}

fn new_session(app: &App) -> NodeBuilder {
    let open = app.clone();
    list_row(false, NEW_SESSION, "")
        .child(icons::plus())
        .pointer_down(move |event| {
            open.open_session(Open::New);
            event.stop_propagation();
        })
}

fn projects(live: &[Live]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for entry in live {
        let name = project_of(entry);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn project_of(entry: &Live) -> String {
    entry.workspace.file_name().map_or_else(
        || entry.workspace.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn live_row(app: &App, entry: &Live) -> NodeBuilder {
    let title = if entry.title.trim().is_empty() {
        UNTITLED.to_string()
    } else {
        crate::text::ellipsize(entry.title.trim(), PREVIEW_CELLS)
    };
    let open = app.clone();
    let (id, active) = (entry.session.clone(), entry.active);
    list_row(active, title, entry.status.label())
        .child(dot(tone(entry.status)))
        .pointer_down(move |event| {
            if !active {
                open.open_session(Open::Resume(id.clone()));
            }
            event.stop_propagation();
        })
}

fn past_row(app: &App, info: &session::SessionInfo) -> NodeBuilder {
    let title = info
        .name
        .clone()
        .unwrap_or_else(|| crate::text::ellipsize(info.preview.trim(), PREVIEW_CELLS));
    let title = if title.trim().is_empty() {
        UNTITLED.to_string()
    } else {
        title
    };
    let open = app.clone();
    let id = info.id.clone();
    list_row(false, title, "").pointer_down(move |event| {
        open.open_session(Open::Resume(id.clone()));
        event.stop_propagation();
    })
}

const fn tone(status: Status) -> Tone {
    match status {
        Status::Working => Tone::Primary,
        Status::NeedsYou => Tone::Warn,
        Status::Review => Tone::Success,
        Status::Failed => Tone::Error,
        Status::Idle => Tone::Neutral,
    }
}

fn seat(app: &App) -> NodeBuilder {
    let model = app.model.get().snapshot.model.clone();
    list_row(false, app.info.endpoint.clone(), model).child(icons::avatar())
}
