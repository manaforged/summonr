use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::session::SessionInfo;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeEventsExt, WidgetLayoutExt, container, label, signal,
    text_edit, virtual_list,
};
use sigil::prelude::{NodeBuilder, Signal};

use super::app::App;
use super::canvas;
use super::prelude::*;
use crate::tui::short_session;

#[derive(Clone, Copy)]
pub(super) struct Picker {
    pub(super) open: Signal<bool>,
    pub(super) query: Signal<String>,
    pub(super) caret: Signal<usize>,
    pub(super) selected: Signal<usize>,
    scope: Signal<Scope>,
    current: Signal<String>,
    loading: Signal<bool>,
    choices: Signal<Vec<(SessionInfo, bool)>>,
    error: Signal<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Project,
    All,
    Related,
}

impl Picker {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            query: signal(String::new()),
            caret: signal(0),
            selected: signal(0),
            scope: signal(Scope::Project),
            current: signal(String::new()),
            loading: signal(false),
            choices: signal(Vec::new()),
            error: signal(String::new()),
        }
    }

    pub(super) fn start(self, current: &str, related: bool) {
        self.open.set(true);
        self.query.set(String::new());
        self.caret.set(0);
        self.selected.set(0);
        self.scope.set(if related {
            Scope::Related
        } else {
            Scope::Project
        });
        self.current.set(current.to_string());
        self.loading.set(true);
        self.choices.set(Vec::new());
        self.error.set(String::new());
    }

    pub(super) fn update(self, result: Result<Vec<(SessionInfo, bool)>, String>) {
        self.loading.set(false);
        match result {
            Ok(rows) => {
                self.choices.set(rows);
                if self.scope.peek() == Scope::Related {
                    self.selected.set(
                        self.rows()
                            .iter()
                            .position(|info| info.id == self.current.peek())
                            .unwrap_or(0),
                    );
                }
            }
            Err(error) => self.error.set(error),
        }
    }

    pub(super) fn note(self, text: &str) {
        self.error.set(text.to_string());
    }

    pub(super) fn rows(self) -> Vec<SessionInfo> {
        let query = self.query.get().to_lowercase();
        let scope = self.scope.get();
        let choices = self.choices.get();
        let rows = if scope == Scope::Related {
            self.related(&choices)
        } else {
            choices
                .into_iter()
                .filter(|(info, local)| {
                    info.id != self.current.get() && (scope == Scope::All || *local)
                })
                .map(|(info, _)| info)
                .collect()
        };
        rows.into_iter()
            .filter(|info| {
                let text = format!(
                    "{} {} {} {} {}",
                    info.id,
                    info.name.as_deref().unwrap_or(""),
                    info.cwd.as_deref().unwrap_or(""),
                    info.model.as_deref().unwrap_or(""),
                    info.preview
                )
                .to_lowercase();
                query.split_whitespace().all(|word| text.contains(word))
            })
            .collect()
    }

    fn related(self, choices: &[(SessionInfo, bool)]) -> Vec<SessionInfo> {
        let by_id: HashMap<_, _> = choices
            .iter()
            .map(|(info, _)| (info.id.as_str(), info))
            .collect();
        let current = self.current.get();
        let mut root = current.as_str();
        let mut seen = HashSet::new();
        while seen.insert(root) {
            let Some(parent) = by_id.get(root).and_then(|info| info.parent.as_deref()) else {
                break;
            };
            root = parent;
        }
        seen.clear();
        let mut pending = vec![root];
        let mut rows = Vec::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(info) = by_id.get(id) {
                rows.push((*info).clone());
            }
            pending.extend(
                choices
                    .iter()
                    .rev()
                    .filter(|(info, _)| info.parent.as_deref() == Some(id))
                    .map(|(info, _)| info.id.as_str()),
            );
        }
        rows
    }

    fn title(self, info: &SessionInfo) -> String {
        let text = info.name.as_deref().unwrap_or(&info.preview);
        let text = if text.is_empty() {
            "Untitled session"
        } else {
            text
        };
        if self.scope.get() != Scope::Related {
            return text.to_string();
        }
        let choices = self.choices.get();
        let mut parent = info.parent.as_deref();
        let mut seen = HashSet::new();
        while let Some(id) = parent {
            if !seen.insert(id) {
                break;
            }
            parent = choices
                .iter()
                .find(|(row, _)| row.id == id)
                .and_then(|(row, _)| row.parent.as_deref());
        }
        format!(
            "{}{}{text}",
            "  ".repeat(seen.len().min(4)),
            if info.parent.is_some() { "↳ " } else { "" },
        )
    }

    fn detail(self, info: &SessionInfo) -> String {
        if self.scope.get() == Scope::Related {
            return format!(
                "{}{} · {}",
                short_session(&info.id),
                if info.id == self.current.get() {
                    " (current)"
                } else {
                    ""
                },
                info.parent.as_deref().map_or_else(
                    || "original session".to_string(),
                    |parent| format!("from {}", short_session(parent))
                )
            );
        }
        format!(
            "{} · {} · {}",
            short_session(&info.id),
            info.cwd
                .as_deref()
                .and_then(|cwd| Path::new(cwd).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("unknown project"),
            info.model.as_deref().unwrap_or("unknown model")
        )
    }

    fn heading(self) -> &'static str {
        match self.scope.get() {
            Scope::Project => "Resume · this project",
            Scope::All => "Resume · all projects",
            Scope::Related => "Session tree · related branches",
        }
    }

    fn empty(self) -> String {
        if self.loading.get() {
            return "Loading sessions…".to_string();
        }
        let error = self.error.get();
        if !error.is_empty() {
            return error;
        }
        if self.scope.get() == Scope::Project {
            "No matches here. Tab shows all projects.".to_string()
        } else {
            "No matching sessions. Edit the search.".to_string()
        }
    }

    pub(super) fn shift(self, delta: isize) {
        let last = self.rows().len().saturating_sub(1);
        self.selected.set(
            self.selected
                .peek()
                .min(last)
                .saturating_add_signed(delta)
                .min(last),
        );
    }

    pub(super) fn scope(self) {
        self.scope.set(match self.scope.peek() {
            Scope::Project => Scope::All,
            Scope::All => Scope::Related,
            Scope::Related => Scope::Project,
        });
        self.selected.set(0);
    }
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let picker = app.sessions;
    shell(
        picker.open,
        picker.heading(),
        app.viewport.peek().0,
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .child(
                text_edit(picker.query)
                    .caret_position(picker.caret)
                    .height(canvas::overlay_field()),
            )
            .child(list(app, picker)),
    )
}

fn list(app: App, picker: Picker) -> NodeBuilder {
    let paint = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            let rows = picker.rows();
            let selected = picker.selected.get().min(rows.len().saturating_sub(1));
            if rows.is_empty() {
                return vec![container().child(label(picker.empty()))];
            }
            let source = paint.clone();
            let band = canvas::overlay_list_row();
            let shown = canvas::overlay_shown(rows.len(), paint.viewport.get().1);
            let total = rows.len();
            let mut items = vec![
                container().layout(LayoutStyle::block().fill_width()).child(
                    virtual_list(rows, move |index, info: &SessionInfo| {
                        row(source.clone(), picker, index, info, selected)
                    })
                    .row_height(band)
                    .height(band * shown as f32)
                    .viewport_height(band * shown as f32)
                    .follow(paint.sessions.selected),
                ),
            ];
            items.push(hint(
                "↑↓ move · ↵ resume · tab scope · esc close",
                format!("{} / {}", selected + 1, total),
            ));
            items
        })
}

fn row(app: App, picker: Picker, index: usize, info: &SessionInfo, selected: usize) -> NodeBuilder {
    let choose = app.clone();
    let title = picker.title(info);
    let detail = picker.detail(info);
    pick(index == selected, title, detail).pointer_down(move |event| {
        choose.sessions.selected.set(index);
        choose.resume_session();
        event.stop_propagation();
    })
}
