use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use regex::{Regex, RegexBuilder, escape};

use super::app::App;
use super::model::{Entry, Model, entry_markdown};
use crate::session::Prompt;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeEventsExt, NodeStyleExt, Role, WidgetLayoutExt, classes,
    container, label, modal, signal, text_edit, virtual_list,
};
use sigil::prelude::{NodeBuilder, Signal};

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
    container().layout(LayoutStyle::block()).child(
        modal(picker.open).title(picker.title()).trap(true).child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .gap(8.0)
                        .fill_width()
                        .fill_height(),
                )
                .child(
                    text_edit(picker.query)
                        .caret_position(picker.caret)
                        .height(32.0),
                )
                .child(results(app, picker)),
        ),
    )
}

fn results(app: App, picker: Picker) -> NodeBuilder {
    let paint = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            super::view::watch(&paint);
            let warning = picker.warning.get();
            let mut items: Vec<NodeBuilder> = Vec::new();
            if !warning.is_empty() {
                items.push(
                    container().child(label(warning).class(classes!("text-xs text-error-11"))),
                );
            }
            match picker.rows(&paint.model.borrow()) {
                Ok(rows) => {
                    let selected = picker.selected.get().min(rows.len().saturating_sub(1));
                    if rows.is_empty() {
                        items.push(container().child(label("No matches. Edit the search.")));
                    }
                    items.push(container().child(label(format!(
                        "{} match{} · ↑↓ select · enter {} · esc",
                        rows.len(),
                        if rows.len() == 1 { "" } else { "es" },
                        picker.action()
                    ))));
                    let height = if picker.is_history() { 48.0 } else { 28.0 };
                    let source = paint.clone();
                    items.push(
                        container().layout(LayoutStyle::block().fill_width()).child(
                            virtual_list(rows, move |index, (entry, preview): &(usize, String)| {
                                row(
                                    source.clone(),
                                    picker,
                                    index,
                                    *entry,
                                    preview,
                                    selected,
                                    height,
                                )
                            })
                            .row_height(height)
                            .viewport_fill()
                            .scroll_into_view(selected),
                        ),
                    );
                }
                Err(error) => items.push(container().child(label(error))),
            }
            items
        })
}

fn row(
    app: App,
    picker: Picker,
    index: usize,
    entry: usize,
    preview: &str,
    selected: usize,
    height: f32,
) -> NodeBuilder {
    let choose = app.clone();
    let title = if picker.is_history() {
        preview.to_string()
    } else {
        format!("{}  {preview}", entry + 1)
    };
    let detail = picker.detail(entry);
    let mut item = container()
        .role(Role::Button)
        .label(title.clone())
        .focusable(true)
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .height(height)
                .padding_x(8.0)
                .gap(4.0),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(index == selected), "bg-gray-5")
        .pointer_down(move |event| {
            choose.search.selected.set(index);
            choose.jump();
            event.stop_propagation();
        })
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
}
