use sigil::prelude::Paint;
use sigil::widgets::CursorEdge;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::session::SessionInfo;
use crate::tui::matching::all_words;
use sigil::prelude::{NodeBuilder, Signal};
use sigil::prelude::{container, signal};

use super::app::App;
use super::canvas;
use super::ui::{Bindings, Dialog, Hints, Key, List, Size, alt, filter_field};

mod resume_row;
use resume_row::row;

const CURRENT: &str = " (current)";

#[derive(Clone, Copy)]
pub(super) struct Picker {
    pub(super) open: Signal<bool>,
    pub(super) list: List,
    scope: Signal<Scope>,
    current: Signal<String>,
    loading: Signal<bool>,
    choices: Signal<Vec<(SessionInfo, bool)>>,
    error: Signal<String>,
    pub(super) armed: Signal<Option<(String, std::time::Instant)>>,
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
            list: List::new(),
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
        self.list.type_query(String::new());
        self.list.select(0);
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
            self.list.select(
                self.rows()
                    .iter()
                    .position(|info| info.id == self.current.peek())
                    .unwrap_or(0),
            );
        }
    }

    pub(super) fn note(self, text: &str) {
        self.error.set(text.to_string());
    }

    pub(super) fn rows(self) -> Vec<SessionInfo> {
        let query = self.list.query.get();
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
                );
                all_words(&text, &query)
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
        self.list.shift(delta, self.rows().len(), CursorEdge::Clamp);
    }

    pub(super) fn scope(self) {
        self.armed.set(None);
        self.error.set(String::new());
        self.scope.set(match self.scope.peek() {
            Scope::Project => Scope::All,
            Scope::All => Scope::Related,
            Scope::Related => Scope::Project,
        });
        self.list.select(0);
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
    let viewport = app.viewport;
    let source = app.clone();
    Dialog::new(Size::Normal, picker.heading(), Hints::new())
        .live_hints(move || {
            Hints::new()
                .key(Key::UpDown, "move")
                .key(Key::Enter, "resume")
                .key(Key::Tab, "scope")
                .key(Key::Alt('d'), "delete")
                .key(Key::Esc, "close")
                .count(picker.list.cursor.get(), picker.rows().len())
        })
        .child(filter_field(&app, picker.list.query, picker.list.caret))
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
        .collection(
            picker.list,
            move || Ok(picker.rows()),
            move || picker.empty(),
            move |len| {
                (len > 0).then(|| {
                    (
                        canvas::overlay_list_row(),
                        canvas::overlay_shown(len, viewport.get().1),
                    )
                })
            },
            move |info, selected| row(&source, picker, info, selected),
            move || app.resume_session(),
        )
        .build(picker.open, cols, &keys)
}
