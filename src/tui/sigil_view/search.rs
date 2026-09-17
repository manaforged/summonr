use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use regex::{Regex, RegexBuilder, escape};

use super::app::App;
use super::canvas::{Cell, palette};
use super::chat::{accent, draft_view, ellipsize, write_at};
use super::hit::Target;
use super::model::{Entry, Model, entry_markdown};
use crate::session::Prompt;
#[cfg(feature = "gui")]
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeStyleExt, WidgetLayoutExt, classes, container, label,
    virtual_list,
};
use sigil::prelude::{NodeBuilder, Signal, signal, text_edit};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Conversation,
    Prompts,
    Branch,
}

#[derive(Clone, Copy)]
pub(super) struct Picker {
    pub(super) open: Signal<bool>,
    pub(super) query: Signal<String>,
    pub(super) caret: Signal<usize>,
    pub(super) selected: Signal<usize>,
    pub(super) target: Signal<Option<usize>>,
    mode: Signal<Mode>,
    saved: Signal<Result<Arc<Vec<Prompt>>, String>>,
    warning: Signal<String>,
}

impl Picker {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            query: signal(String::new()),
            caret: signal(0),
            selected: signal(0),
            target: signal(None),
            mode: signal(Mode::Conversation),
            saved: signal(Ok(Arc::new(Vec::new()))),
            warning: signal(String::new()),
        }
    }

    pub(super) fn start(self, query: &str) {
        self.show(Mode::Conversation, query);
    }

    pub(super) fn history(self, query: &str) {
        self.saved.set(Err("Loading saved prompts…".to_string()));
        self.show(Mode::Prompts, query);
    }

    pub(super) fn branch(self, query: &str) {
        self.saved.set(Err("Loading completed turns…".to_string()));
        self.show(Mode::Branch, query);
    }

    fn show(self, mode: Mode, query: &str) {
        self.mode.set(mode);
        self.query.set(query.to_string());
        self.caret.set(query.len());
        self.selected.set(0);
        self.warning.set(String::new());
        self.open.set(true);
    }

    pub(super) fn update(self, result: Result<(Vec<Prompt>, usize), String>) {
        self.warning.set(String::new());
        self.saved.set(result.map(|(prompts, skipped)| {
            if skipped > 0 {
                self.warning
                    .set(format!("{skipped} skipped · see transcript"));
            }
            Arc::new(prompts)
        }));
    }

    pub(super) fn is_history(self) -> bool {
        self.mode.get() != Mode::Conversation
    }

    pub(super) fn is_branch(self) -> bool {
        self.mode.get() == Mode::Branch
    }

    pub(super) fn prompt(self, index: usize) -> Option<Prompt> {
        self.saved.peek().ok()?.get(index).cloned()
    }

    fn title(self) -> &'static str {
        match self.mode.get() {
            Mode::Conversation => "Search conversation",
            Mode::Prompts => "Search saved prompts",
            Mode::Branch => "Branch after a turn",
        }
    }

    fn action(self) -> &'static str {
        match self.mode.get() {
            Mode::Conversation => "jump",
            Mode::Prompts => "insert text",
            Mode::Branch => "branch",
        }
    }

    fn detail(self, index: usize) -> String {
        let Some(prompt) = self.prompt(index).filter(|_| self.is_history()) else {
            return String::new();
        };
        let session = &prompt.session;
        if self.is_branch() {
            return format!("After turn {} · files unchanged", prompt.turn + 1);
        }
        let name = session
            .name
            .as_deref()
            .unwrap_or_else(|| crate::session::short(&session.id));
        let project = session
            .cwd
            .as_deref()
            .and_then(|cwd| Path::new(cwd).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("unknown project");
        format!("{name} · {project} · turn {}", prompt.turn + 1)
    }

    pub(super) fn pattern(self) -> Result<Regex, String> {
        let query = self.query.get();
        RegexBuilder::new(&escape(query.trim()))
            .case_insensitive(true)
            .build()
            .map_err(|error| format!("Cannot search: {error}"))
    }

    pub(super) fn rows(self, model: &Model) -> Result<Vec<(usize, String)>, String> {
        let pattern = self.pattern()?;
        if self.is_history() {
            let saved = self.saved.get()?;
            let mut seen = HashSet::new();
            return Ok(saved
                .iter()
                .enumerate()
                .filter_map(|(index, prompt)| {
                    if self.is_branch() && (!prompt.closed || prompt.session.id != model.session) {
                        return None;
                    }
                    if prompt.text.is_empty() && !self.is_branch() {
                        return None;
                    }
                    let context = format!(
                        "{} {} {}",
                        prompt.session.name.as_deref().unwrap_or(""),
                        prompt.session.id,
                        prompt.session.cwd.as_deref().unwrap_or("")
                    );
                    let preview = snippet(&pattern, &prompt.text).or_else(|| {
                        pattern.is_match(&context).then(|| {
                            prompt
                                .text
                                .chars()
                                .take(180)
                                .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
                                .collect()
                        })
                    })?;
                    let preview = if prompt.text.is_empty() {
                        format!("{} attached image(s)", prompt.images)
                    } else {
                        preview
                    };
                    (self.is_branch() || seen.insert(prompt.text.as_str()))
                        .then_some((index, preview))
                })
                .collect());
        }
        Ok(model
            .entries
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(index, entry)| {
                let text = match entry {
                    Entry::User(text) | Entry::Thinking(text) | Entry::Assistant { text, .. } => {
                        text.clone()
                    }
                    _ => entry_markdown(entry),
                };
                snippet(&pattern, &text).map(|preview| (index, preview))
            })
            .collect())
    }

    pub(super) fn shift(self, model: &Model, delta: isize) {
        let last = self
            .rows(model)
            .map_or(0, |rows| rows.len().saturating_sub(1));
        self.selected
            .set(self.selected.peek().saturating_add_signed(delta).min(last));
    }
}

fn snippet(pattern: &Regex, text: &str) -> Option<String> {
    let found = pattern.find(text)?;
    let start = text[..found.start()]
        .char_indices()
        .rev()
        .nth(24)
        .map_or(0, |(byte, _)| byte);
    let preview: String = text[start..]
        .chars()
        .take(180)
        .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
        .collect();
    Some(format!("{}{}", if start > 0 { "…" } else { "" }, preview))
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let picker = app.search;
    let input = text_edit(picker.query)
        .caret_position(picker.caret)
        .change(move |_| picker.selected.set(0))
        .class("w-full h-full");
    super::settings::layer(app, grid, input)
}

#[cfg(feature = "gui")]
pub(super) fn desktop(app: App) -> NodeBuilder {
    let _revision = app.panes.get();
    let picker = app.search;
    let close = app.clone();
    let mut body = container()
        .layout(LayoutStyle::flex_column().fill().padding(24.0).gap(16.0))
        .class(classes!("bg-gray-1"))
        .child(label(picker.title()).class(classes!("text-lg text-gray-12")))
        .child(
            text_edit(picker.query)
                .caret_position(picker.caret)
                .change(move |_| picker.selected.set(0))
                .height(48.0),
        );
    let rows = picker.rows(&app.model.borrow());
    let warning = picker.warning.get();
    if !warning.is_empty() {
        body = body.child(label(warning).class(classes!("text-xs text-error-11")));
    }
    match rows {
        Ok(rows) => {
            let selected = picker.selected.get().min(rows.len().saturating_sub(1));
            if rows.is_empty() {
                body = body.child(label("No matches. Edit the search."));
            }
            body = body.child(label(format!(
                "{} match{} · ↑↓ select · enter {} · esc",
                rows.len(),
                if rows.len() == 1 { "" } else { "es" },
                picker.action()
            )));
            let height = if picker.is_history() { 48.0 } else { 28.0 };
            let results = virtual_list(rows, move |index, (entry, preview): &(usize, String)| {
                let choose = app.clone();
                let title = if picker.is_history() {
                    preview.clone()
                } else {
                    format!("{}  {preview}", entry + 1)
                };
                let detail = picker.detail(*entry);
                let mut item = super::desktop::hit(title.clone(), index == selected, move || {
                    choose.search.selected.set(index);
                    choose.jump();
                })
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .height(height)
                        .padding_x(8.0)
                        .gap(4.0),
                )
                .child(
                    label(title)
                        .ellipsize()
                        .class(classes!("text-sm text-gray-12")),
                );
                if !detail.is_empty() {
                    item = item.child(
                        label(detail)
                            .ellipsize()
                            .class(classes!("text-xs text-gray-11")),
                    );
                }
                item
            })
            .row_height(height)
            .viewport_fill()
            .scroll_into_view(selected);
            body = body.child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .grow(1.0)
                            .min_size(0.0, 0.0),
                    )
                    .child(results),
            );
        }
        Err(error) => body = body.child(label(error)),
    }
    body.child(
        super::desktop::hit("Close search".to_string(), false, move || {
            close.search.open.set(false);
        })
        .child(label("Close search")),
    )
}

fn grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    app.hits.clear();
    let left = usize::from(width > 2);
    let columns = width.saturating_sub(left * 2);
    let model = app.model.borrow();
    let color = accent(&model.prefs.theme);
    write_at(
        &mut cells,
        width,
        left,
        1,
        &ellipsize(app.search.title(), columns),
        color,
    );
    let query = draft_view(
        &app.search.query.get(),
        app.search.caret.get(),
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
    let (rows, note) = match app.search.rows(&model) {
        Ok(rows) => (rows, "No matches. Edit the search.".to_string()),
        Err(error) => (Vec::new(), error),
    };
    let warning = app.search.warning.get();
    let top = if warning.is_empty() { 5 } else { 7 };
    if !warning.is_empty() {
        write_at(
            &mut cells,
            width,
            left,
            5,
            &ellipsize(&warning, columns),
            palette().dim,
        );
    }
    if rows.is_empty() {
        write_at(
            &mut cells,
            width,
            left,
            top,
            &ellipsize(&note, columns),
            palette().dim,
        );
    }
    let selected = app.search.selected.get().min(rows.len().saturating_sub(1));
    let stride = if app.search.is_history() { 2 } else { 1 };
    let count = height.saturating_sub(top + 2) / stride;
    let start = super::hit::window(app, selected, rows.len(), count);
    for (offset, (entry, preview)) in rows.iter().enumerate().skip(start).take(count) {
        let y = top + (offset - start) * stride;
        let marker = if offset == selected { "›" } else { " " };
        let label = if app.search.is_history() {
            format!("{marker} {preview}")
        } else {
            format!("{marker} {}  {preview}", entry + 1)
        };
        write_at(
            &mut cells,
            width,
            left,
            y,
            &ellipsize(&label, columns),
            if offset == selected {
                color
            } else {
                palette().text
            },
        );
        app.hits
            .add(y, left, left + columns, Target::Search(offset));
        if stride == 2 {
            write_at(
                &mut cells,
                width,
                left + 2,
                y + 1,
                &ellipsize(&app.search.detail(*entry), columns.saturating_sub(2)),
                palette().dim,
            );
            app.hits
                .add(y + 1, left, left + columns, Target::Search(offset));
        }
    }
    let help = if width < 60 {
        format!("{} · ↑↓ · enter {} · esc", rows.len(), app.search.action())
    } else {
        format!(
            "{} match{} · ↑↓ select · enter {} · esc",
            rows.len(),
            if rows.len() == 1 { "" } else { "es" },
            app.search.action()
        )
    };
    write_at(
        &mut cells,
        width,
        left,
        height.saturating_sub(1),
        &ellipsize(&help, columns),
        palette().dim,
    );
    cells
}
