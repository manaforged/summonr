use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, ListRowSlots, MenuItem, NodeBuilder, Signal, Tone, container,
    context_menu, dot, list_row_with, signal,
};

use super::{App, UNTITLED, section};
use crate::session::{self, SessionInfo};
use crate::tui::hub::{Live, Open, Status};

const PREVIEW_CELLS: usize = 36;
const RECENT: usize = 8;
const RENAME: &str = "Rename";
const DELETE: &str = "Delete";
const NOW: &str = "now";
const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

pub(super) fn sessions(app: &App) -> NodeBuilder {
    let live = {
        let model = app.model;
        sigil::reactive::derived(move || model.get().live.clone())
    };
    let here = app.workspace.display().to_string();
    let recent = recent(session::list().unwrap_or_default(), &here);
    let gone: Signal<HashSet<String>> = signal(HashSet::new());
    let open = app.clone();
    container()
        .layout(LayoutStyle::flex_column().fill_width().gap(2.0))
        .children_reactive(move || {
            let live = live.get();
            let mut rows = Vec::new();
            let projects = projects(&live);
            let titles = titles(&live);
            for project in &projects {
                if projects.len() > 1 {
                    rows.push(section(project.clone()));
                }
                rows.extend(
                    live.iter()
                        .zip(&titles)
                        .filter(|(entry, _)| project_of(entry) == *project)
                        .map(|(entry, title)| live_row(&open, entry, title.clone())),
                );
            }
            let mut skip = gone.get();
            skip.extend(live.iter().map(|entry| entry.session.clone()));
            let past: Vec<NodeBuilder> = recent
                .iter()
                // complexity: bounded by HashSet lookup
                .filter(|info| !skip.contains(&info.id))
                .take(RECENT)
                .map(|info| past_row(&open, info, gone))
                .collect();
            if !past.is_empty() {
                rows.push(section("Recent"));
                rows.extend(past);
            }
            rows
        })
}

pub(in crate::tui::sigil_view::desktop) fn recent(
    sessions: Vec<SessionInfo>,
    here: &str,
) -> Vec<SessionInfo> {
    let mut seen = HashSet::new();
    let mut recent: Vec<SessionInfo> = sessions
        .into_iter()
        .filter(|info| info.cwd.as_deref() == Some(here) && info.depth == 0)
        .filter(|info| !info.preview.trim().is_empty())
        .collect();
    recent.reverse();
    recent.retain(|info| seen.insert(past_title(info)));
    recent
}

fn projects(live: &[Live]) -> Vec<String> {
    let mut seen = HashSet::new();
    live.iter()
        .map(project_of)
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

fn project_of(entry: &Live) -> String {
    entry.workspace.file_name().map_or_else(
        || entry.workspace.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn titled(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        UNTITLED.to_string()
    } else {
        crate::text::ellipsize(text, PREVIEW_CELLS)
    }
}

fn titles(live: &[Live]) -> Vec<String> {
    let base: Vec<String> = live.iter().map(|entry| titled(&entry.title)).collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for title in &base {
        *counts.entry(title.as_str()).or_default() += 1;
    }
    base.iter()
        .zip(live)
        .map(|(title, entry)| {
            if counts.get(title.as_str()).copied().unwrap_or(0) > 1 {
                format!("{title} · {}", session::short(&entry.session))
            } else {
                title.clone()
            }
        })
        .collect()
}

fn past_title(info: &SessionInfo) -> String {
    titled(info.name.as_deref().unwrap_or(&info.preview))
}

fn live_row(app: &App, entry: &Live, title: String) -> NodeBuilder {
    let open = app.clone();
    let (id, active) = (entry.session.clone(), entry.active);
    let label = entry.status.label();
    let target = id.clone();
    let row = list_row_with(
        active,
        title,
        ListRowSlots {
            leading: Some(Box::new(dot(tone(entry.status)))),
            status: (!label.is_empty()).then(|| label.into()),
            activate: Some(Rc::new(move || {
                if !active {
                    open.open_session(Open::Resume(target.clone()));
                }
            })),
            ..ListRowSlots::default()
        },
    );
    let menu = app.clone();
    context_menu(row, move || vec![rename(&menu, &id)])
}

fn past_row(app: &App, info: &SessionInfo, gone: Signal<HashSet<String>>) -> NodeBuilder {
    let open = app.clone();
    let id = info.id.clone();
    let target = id.clone();
    let row = list_row_with(
        false,
        past_title(info),
        ListRowSlots {
            status: Some(age(info).into()),
            activate: Some(Rc::new(move || {
                open.open_session(Open::Resume(target.clone()));
            })),
            ..ListRowSlots::default()
        },
    );
    let menu = app.clone();
    context_menu(row, move || {
        let (remove, target) = (menu.clone(), id.clone());
        vec![
            rename(&menu, &id),
            MenuItem::new(DELETE).activate(move || {
                if remove.remove_session(&target, DELETE) {
                    gone.update(|ids| {
                        ids.insert(target.clone());
                    });
                }
            }),
        ]
    })
}

fn rename(app: &App, id: &str) -> MenuItem {
    let (app, id) = (app.clone(), id.to_string());
    MenuItem::new(RENAME).activate(move || app.rename_session(&id))
}

fn age(info: &SessionInfo) -> String {
    let Some(secs) = std::fs::metadata(&info.path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .map(|elapsed| elapsed.as_secs())
    else {
        return String::new();
    };
    match secs {
        secs if secs < MINUTE => NOW.to_string(),
        secs if secs < HOUR => format!("{}m", secs / MINUTE),
        secs if secs < DAY => format!("{}h", secs / HOUR),
        secs => format!("{}d", secs / DAY),
    }
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
