use sigil::prelude::Paint;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use regex::{Regex, RegexBuilder, escape};

use super::app::App;
use super::canvas;
use super::model::{Entry, Model, entry_markdown};
use super::ui::{Dialog, Hints, Key, Size, pick};
use crate::session::Prompt;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeEventsExt, WidgetLayoutExt, container, signal, text_edit,
    virtual_list,
};
use sigil::prelude::{NodeBuilder, Signal};

const SNIPPET_CELLS: usize = 180;

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

    fn hints(self, model: &Model) -> Hints {
        let count = match self.rows(model) {
            Ok(rows) if !rows.is_empty() => {
                let selected = self.selected.get().min(rows.len() - 1);
                format!("{} / {}", selected + 1, rows.len())
            }
            Ok(_) | Err(_) => String::new(),
        };
        let action = match self.mode.get() {
            Mode::Conversation => "jump",
            Mode::Prompts => "insert",
            Mode::Branch => "branch",
        };
        Hints::new()
            .key(Key::UpDown, "select")
            .key(Key::Enter, action)
            .key(Key::Esc, "close")
            .right(count)
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
                    let preview = snippet(&pattern, &prompt.text)
                        .or_else(|| pattern.is_match(&context).then(|| one_line(&prompt.text)))?;
                    let preview = if prompt.text.is_empty() {
                        format!("{} attached", crate::text::plural(prompt.images, "image"))
                    } else {
                        preview
                    };
                    (self.is_branch() || seen.insert(prompt.text.as_str()))
                        .then_some((index, preview))
                })
                .collect());
        }
        let hidden = model.prefs.reasoning == crate::prefs::Visibility::Off;
        Ok(model
            .entries
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, entry)| !(hidden && matches!(entry, Entry::Thinking(_))))
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
    let preview = one_line(&text[start..]);
    Some(format!("{}{}", if start > 0 { "…" } else { "" }, preview))
}

fn one_line(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
        .collect();
    crate::text::ellipsize(&flat, SNIPPET_CELLS)
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let picker = app.search;
    let cols = app.viewport.peek().0;
    let keys = app.model.peek().prefs.hotkeys.clone();
    let live = app.clone();
    Dialog::new(Size::Normal, picker.title(), Hints::new())
        .live_hints(move || picker.hints(&live.model.get()))
        .child(
            text_edit(picker.query)
                .caret_position(picker.caret)
                .height(canvas::overlay_field()),
        )
        .child(results(app, picker))
        .build(picker.open, cols, &keys)
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
            let warning = picker.warning.get();
            let mut items: Vec<NodeBuilder> = Vec::new();
            if !warning.is_empty() {
                items.push(
                    container().child(
                        sigil::kernel::text(warning)
                            .nowrap()
                            .text_color(sigil::prelude::Token::OnSurface)
                            .font_size(12.0)
                            .text_color(sigil::prelude::Token::Error11),
                    ),
                );
            }
            match picker.rows(&paint.model.get()) {
                Ok(rows) => {
                    let selected = picker.selected.get().min(rows.len().saturating_sub(1));
                    if rows.is_empty() {
                        items.push(
                            container().child(
                                sigil::kernel::text("no matches · edit the search")
                                    .nowrap()
                                    .text_color(sigil::prelude::Token::OnSurface),
                            ),
                        );
                    }
                    let height = if picker.is_history() {
                        canvas::overlay_list_row() * 2.0
                    } else {
                        canvas::overlay_list_row()
                    };
                    let source = paint.clone();
                    items.push(
                        container().layout(LayoutStyle::block().fill_width()).child(
                            virtual_list(rows, move |index, (entry, preview): &(usize, String)| {
                                row(source.clone(), picker, index, *entry, preview, selected)
                            })
                            .row_height(height)
                            .height(height * 10.0)
                            .viewport_height(height * 10.0)
                            .follow_row(picker.selected),
                        ),
                    );
                }
                Err(error) => items.push(
                    container().child(
                        sigil::kernel::text(error)
                            .nowrap()
                            .text_color(sigil::prelude::Token::OnSurface),
                    ),
                ),
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
) -> NodeBuilder {
    let choose = app.clone();
    let title = preview.to_string();
    let detail = picker.detail(entry);
    pick(index == selected, title, detail).pointer_down(move |event| {
        choose.search.selected.set(index);
        choose.jump();
        event.stop_propagation();
    })
}
