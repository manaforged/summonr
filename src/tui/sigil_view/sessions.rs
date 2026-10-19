use sigil::prelude::Paint;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::session::SessionInfo;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, WidgetLayoutExt, container, signal, text_edit, virtual_list,
};
use sigil::prelude::{NodeBuilder, Signal};

use super::app::App;
use super::canvas;
use super::ui::{Bindings, Dialog, Hints, Key, Size, alt};

mod resume_row;
use resume_row::row;

const CURRENT: &str = " (current)";

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
    armed: Signal<Option<(String, std::time::Instant)>>,
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
            armed: signal(None),
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
        self.armed.set(None);
    }

    pub(super) fn update(self, result: Result<Vec<(SessionInfo, bool)>, String>) {
        self.loading.set(false);
        match result {
            Ok(rows) => {
                self.choices.set(rows);
                self.focus();
            }
            Err(error) => self.error.set(error),
        }
    }

    fn focus(self) {
        if self.scope.peek() == Scope::Related {
            self.selected.set(
                self.rows()
                    .iter()
                    .position(|info| info.id == self.current.peek())
                    .unwrap_or(0),
            );
        }
    }

    pub(super) fn arm(self, id: &str) -> bool {
        let now = std::time::Instant::now();
        let window = crate::limits::limits().repeat_window();
        let repeat = self
            .armed
            .peek()
            .is_some_and(|(armed, at)| armed == id && now.duration_since(at) <= window);
        self.armed.set(if repeat {
            None
        } else {
            Some((id.to_string(), now))
        });
        repeat
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
                .filter(|(info, local)| scope == Scope::All || *local || self.is_current(info))
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
                crate::session::short(&info.id),
                self.current_mark(info),
                info.parent.as_deref().map_or_else(
                    || "original session".to_string(),
                    |parent| format!("from {}", crate::session::short(parent))
                )
            );
        }
        format!(
            "{}{} · {} · {}",
            crate::session::short(&info.id),
            self.current_mark(info),
            info.cwd
                .as_deref()
                .and_then(|cwd| Path::new(cwd).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("unknown project"),
            info.model.as_deref().unwrap_or("unknown model")
        )
    }

    fn is_current(self, info: &SessionInfo) -> bool {
        info.id == self.current.get()
    }

    fn current_mark(self, info: &SessionInfo) -> &'static str {
        if self.is_current(info) { CURRENT } else { "" }
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
        if self.armed.peek().is_some() {
            self.armed.set(None);
            self.error.set(String::new());
        }
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
        self.armed.set(None);
        self.error.set(String::new());
        self.scope.set(match self.scope.peek() {
            Scope::Project => Scope::All,
            Scope::All => Scope::Related,
            Scope::Related => Scope::Project,
        });
        self.selected.set(0);
        self.focus();
    }
}

pub(super) fn keys() -> Bindings {
    alt("d", |app, event| {
        event.stop_propagation();
        app.delete_session();
    })
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let picker = app.sessions;
    let cols = app.viewport.peek().0;
    let keys = app.model.peek().prefs.hotkeys.clone();
    Dialog::new(Size::Normal, picker.heading(), Hints::new())
        .live_hints(move || {
            let total = picker.rows().len();
            let count = if total == 0 {
                String::new()
            } else {
                format!("{} / {}", picker.selected.get().min(total - 1) + 1, total)
            };
            Hints::new()
                .key(Key::UpDown, "move")
                .key(Key::Enter, "resume")
                .key(Key::Tab, "scope")
                .key(Key::Alt('d'), "delete")
                .key(Key::Esc, "close")
                .right(count)
        })
        .child(
            text_edit(picker.query)
                .caret_position(picker.caret)
                .placeholder(super::ui::FILTER_HINT)
                .height(canvas::overlay_list_row()),
        )
        .child(container().children_reactive(move || {
            let note = picker.error.get();
            if note.is_empty() || picker.rows().is_empty() {
                Vec::new()
            } else {
                vec![
                    container().child(
                        sigil::kernel::text(note)
                            .wrap()
                            .text_color(sigil::prelude::Token::Warn11),
                    ),
                ]
            }
        }))
        .child(list(app, picker))
        .build(picker.open, cols, &keys)
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
                return vec![
                    container().child(
                        sigil::kernel::text(picker.empty())
                            .nowrap()
                            .text_color(sigil::prelude::Token::OnSurface),
                    ),
                ];
            }
            let source = paint.clone();
            let band = canvas::overlay_list_row();
            let shown = canvas::overlay_shown(rows.len(), paint.viewport.get().1);
            vec![
                container().layout(LayoutStyle::block().fill_width()).child(
                    virtual_list(rows, move |index, info: &SessionInfo| {
                        row(source.clone(), picker, index, info, selected)
                    })
                    .row_height(band)
                    .height(band * shown as f32)
                    .viewport_height(band * shown as f32)
                    .follow_row(paint.sessions.selected),
                ),
            ]
        })
}
