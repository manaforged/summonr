use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::session::SessionInfo;
#[cfg(feature = "gui")]
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeStyleExt, WidgetLayoutExt, classes, container, label,
    virtual_list,
};
use sigil::prelude::{NodeBuilder, Signal, signal, text_edit};

use super::super::short_session;
use super::app::App;
use super::canvas::{Cell, palette};
use super::chat::{accent, draft_view, ellipsize, write_at};
use super::hit::Target;

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
    let input = text_edit(picker.query)
        .caret_position(picker.caret)
        .change(move |_| picker.selected.set(0))
        .class("w-full h-full");
    super::settings::layer(app, grid, input)
}

#[cfg(feature = "gui")]
pub(super) fn desktop(app: App) -> NodeBuilder {
    let picker = app.sessions;
    let rows = picker.rows();
    let selected = picker.selected.get().min(rows.len().saturating_sub(1));
    let close = app.clone();
    let mut body = container()
        .layout(LayoutStyle::flex_column().fill().padding(24.0).gap(16.0))
        .class(classes!("bg-gray-1"))
        .child(label(picker.heading()).class(classes!("text-lg text-gray-12")))
        .child(
            text_edit(picker.query)
                .caret_position(picker.caret)
                .change(move |_| picker.selected.set(0))
                .height(48.0),
        )
        .child(label(
            "↑↓ select · enter resume · tab project/all/tree · ctrl+d delete · esc",
        ));
    if rows.is_empty() {
        body = body.child(label(picker.empty()));
    }
    let results = virtual_list(rows, move |index, info: &SessionInfo| {
        let choose = app.clone();
        let title = picker.title(info);
        super::desktop::hit(title.clone(), index == selected, move || {
            choose.sessions.selected.set(index);
            choose.resume_session();
        })
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .height(48.0)
                .padding_x(8.0)
                .gap(4.0),
        )
        .child(
            label(title)
                .ellipsize()
                .class(classes!("text-sm text-gray-12")),
        )
        .child(
            label(picker.detail(info))
                .ellipsize()
                .class(classes!("text-xs text-gray-11")),
        )
    })
    .row_height(48.0)
    .viewport_fill()
    .scroll_into_view(selected);
    body.child(
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .grow(1.0)
                    .min_size(0.0, 0.0),
            )
            .child(results),
    )
    .child(
        super::desktop::hit("Close sessions".to_string(), false, move || {
            close.sessions.open.set(false)
        })
        .child(label("Close sessions")),
    )
}

fn grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    app.hits.clear();
    if width == 0 || height == 0 {
        return cells;
    }
    let picker = app.sessions;
    let color = accent(&app.model.borrow().prefs.theme);
    let columns = width.saturating_sub(4).min(96);
    let left = width.saturating_sub(columns) / 2;
    write_at(
        &mut cells,
        width,
        left,
        1,
        &ellipsize(picker.heading(), columns),
        color,
    );
    let query = draft_view(
        &picker.query.get(),
        picker.caret.get(),
        columns.saturating_sub(7),
        '▌',
    );
    write_at(
        &mut cells,
        width,
        left,
        3,
        &ellipsize(&format!("find   {query}"), columns),
        palette().text,
    );
    let rows = picker.rows();
    let selected = picker.selected.get().min(rows.len().saturating_sub(1));
    let count = height.saturating_sub(6) / 2;
    let start = super::hit::window(app, selected, rows.len(), count);
    for (offset, info) in rows.iter().enumerate().skip(start).take(count) {
        let y = 5 + (offset - start) * 2;
        let title = picker.title(info);
        let marker = if offset == selected { "›" } else { " " };
        write_at(
            &mut cells,
            width,
            left,
            y,
            &ellipsize(&format!("{marker} {title}"), columns),
            if offset == selected {
                color
            } else {
                palette().text
            },
        );
        let detail = format!("  {}", picker.detail(info));
        write_at(
            &mut cells,
            width,
            left,
            y + 1,
            &ellipsize(&detail, columns),
            palette().dim,
        );
        app.hits
            .add(y, left, left + columns, Target::Session(offset));
        app.hits
            .add(y + 1, left, left + columns, Target::Session(offset));
    }
    if rows.is_empty() {
        write_at(
            &mut cells,
            width,
            left,
            5,
            &ellipsize(&picker.empty(), columns),
            palette().dim,
        );
    }
    let help = if width < 60 {
        "↑↓ move · enter · tab scope · esc"
    } else {
        "↑↓ select · enter resume · tab project/all/tree · ctrl+d delete · esc cancel"
    };
    write_at(
        &mut cells,
        width,
        left,
        height.saturating_sub(1),
        &ellipsize(help, columns),
        palette().dim,
    );
    cells
}
