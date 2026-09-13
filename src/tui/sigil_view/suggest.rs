use sigil::prelude::*;
use sigil_tui::cell_width;
use unicode_segmentation::UnicodeSegmentation;

use super::app::App;
use crate::tui::matching_commands;

const LIMIT: usize = 9;
const MIN_WIDTH: usize = 34;

#[derive(Clone, Copy)]
pub(super) struct Picker {
    cursor: Signal<usize>,
    hidden: Signal<bool>,
    query: Signal<String>,
}

impl Picker {
    pub(super) fn new() -> Self {
        Self {
            cursor: signal(0),
            hidden: signal(false),
            query: signal(String::new()),
        }
    }

    pub(super) fn change(self, draft: &str) {
        self.cursor.set(0);
        self.hidden.set(false);
        self.query.set(draft.to_string());
    }

    pub(super) fn mentions(self) -> bool {
        self.query.peek().contains('@')
    }

    pub(super) fn open(self, draft: &str, caret: usize, files: &[String]) -> bool {
        !self.hidden.get() && !self.rows(draft, caret, files).is_empty()
    }

    pub(super) fn hide(self, draft: &str, caret: usize, files: &[String]) -> bool {
        if !self.open(draft, caret, files) {
            return false;
        }
        self.hidden.set(true);
        true
    }

    pub(super) fn shift(self, draft: &str, caret: usize, files: &[String], delta: isize) -> bool {
        let count = self.rows(draft, caret, files).len();
        if count == 0 || self.hidden.peek() {
            return false;
        }
        let cursor = self.cursor.peek().min(count - 1);
        let next = if delta < 0 {
            let amount = delta.unsigned_abs() % count;
            (cursor + count - amount) % count
        } else {
            (cursor + delta as usize) % count
        };
        self.cursor.set(next);
        true
    }

    pub(super) fn accept(self, draft: &str, caret: usize, files: &[String]) -> Option<String> {
        if self.hidden.peek() {
            return None;
        }
        let rows = self.rows(draft, caret, files);
        let row = rows.get(self.cursor.peek().min(rows.len().saturating_sub(1)))?;
        (row.0 != draft).then(|| row.0.clone())
    }

    pub(super) fn complete(self, draft: &str, caret: usize, files: &[String]) -> Option<String> {
        let rows = self.rows(draft, caret, files);
        if rows.is_empty() || self.hidden.peek() {
            return None;
        }
        let mut cursor = self.cursor.peek().min(rows.len() - 1);
        if rows[cursor].0 == draft && rows.len() > 1 {
            cursor = (cursor + 1) % rows.len();
            self.cursor.set(cursor);
        }
        Some(rows[cursor].0.clone())
    }

    fn cursor(self, count: usize) -> usize {
        self.cursor.get().min(count.saturating_sub(1))
    }

    fn rows(self, draft: &str, caret: usize, files: &[String]) -> Vec<(String, String)> {
        if let Some((_, query)) = mention(draft, caret) {
            let query = query.to_lowercase();
            let mut matches: Vec<_> = files
                .iter()
                .filter_map(|path| {
                    let score = crate::tui::rank(&path.to_lowercase(), "", &query)?;
                    Some((score, path))
                })
                .collect();
            matches.sort_unstable();
            return matches
                .into_iter()
                .map(|(_, path)| (path.clone(), String::new()))
                .collect();
        }
        let query = self.query.get();
        let exact = crate::command_specs().iter().any(|spec| spec.name == draft);
        let rows = if exact && !query.is_empty() {
            matching_commands(&query)
        } else {
            matching_commands(draft)
        };
        rows.into_iter()
            .map(|(name, help)| (name.to_string(), help.to_string()))
            .collect()
    }
}

pub(super) fn mention(draft: &str, caret: usize) -> Option<(usize, &str)> {
    let before = draft.get(..caret)?;
    let start = before.rfind('@')?;
    if start > 0 && !before[..start].ends_with(char::is_whitespace) {
        return None;
    }
    let query = &before[start + 1..];
    if let Some(query) = query.strip_prefix('"') {
        (!query.contains(['"', '\n', '\r'])).then_some((start, query))
    } else {
        (!query.contains(char::is_whitespace)).then_some((start, query))
    }
}

pub(super) fn reference(path: &str) -> String {
    format!("@{}", serde_json::Value::String(path.to_string()))
}

pub(super) struct Menu {
    pub left: usize,
    pub top: usize,
    pub lines: Vec<String>,
}

pub(super) fn menu(app: &App, cols: u16, rows: u16) -> Option<Menu> {
    let draft = app.draft.get();
    let caret = app.caret.get();
    let model = app.model.borrow();
    if app.settings_open.peek()
        || app.auth_open.peek()
        || app.editing_model.peek()
        || model.busy_since.is_some()
        || !app.commands.open(&draft, caret, &model.files)
    {
        return None;
    }
    if cols < 12 || rows < 6 {
        return None;
    }
    let matches = app.commands.rows(&draft, caret, &model.files);
    let cursor = app.commands.cursor(matches.len());
    let splash = app.splash.peek();
    let [left, prompt, anchor_width] = if splash {
        super::scene::prompt_area(app, cols, rows)
    } else {
        [
            2,
            super::chat::prompt_row(rows, &draft),
            cols.saturating_sub(4),
        ]
    };
    let composer = 3_u16;
    let below = splash && rows > prompt.saturating_add(composer).saturating_add(2);
    let room = if below {
        usize::from(rows.saturating_sub(prompt.saturating_add(composer)))
    } else {
        usize::from(prompt.saturating_sub(1))
    };
    let limit = room.saturating_sub(2).min(LIMIT);
    if limit == 0 {
        return None;
    }
    let start = cursor.saturating_add(1).saturating_sub(limit);
    let shown = &matches[start..matches.len().min(start + limit)];
    let height = shown.len() + 2;
    let width = if splash {
        usize::from(anchor_width)
    } else {
        popup_width(&matches, usize::from(cols))
    };
    let top = if below {
        prompt.saturating_add(composer)
    } else {
        prompt.saturating_sub(u16::try_from(height + 1).unwrap_or(u16::MAX))
    };
    let mut lines = Vec::with_capacity(height);
    let title = if mention(&draft, caret).is_some() {
        "files"
    } else {
        "commands"
    };
    lines.push(border(title, width, true));
    for (offset, (name, description)) in shown.iter().enumerate() {
        lines.push(row(name, description, width, start + offset == cursor));
    }
    lines.push(border("↑↓ select · tab accept · esc", width, false));
    Some(Menu {
        left: usize::from(left),
        top: usize::from(top),
        lines,
    })
}

fn popup_width(matches: &[(String, String)], viewport: usize) -> usize {
    let widest = matches
        .iter()
        .map(|(name, description)| cell_width(name.as_str()) + cell_width(description.as_str()) + 7)
        .max()
        .unwrap_or(MIN_WIDTH);
    widest.max(MIN_WIDTH).min(viewport.saturating_sub(4).max(1))
}

fn row(name: &str, description: &str, width: usize, selected: bool) -> String {
    let marker = if selected { '›' } else { ' ' };
    let inner = width.saturating_sub(4);
    let name = fit(name, inner);
    let name_width = cell_width(name.as_str());
    let available = inner.saturating_sub(name_width + 2);
    let help = fit(description, available);
    let gap = inner.saturating_sub(name_width + cell_width(help.as_str()));
    format!("│{marker} {name}{}{help}│", " ".repeat(gap))
}

fn border(label: &str, width: usize, top: bool) -> String {
    let (left, right) = if top { ('╭', '╮') } else { ('╰', '╯') };
    let label = fit(label, width.saturating_sub(4));
    let fill = width.saturating_sub(cell_width(label.as_str()) + 3);
    format!("{left}─{label}{}{right}", "─".repeat(fill))
}

fn fit(text: &str, width: usize) -> String {
    let mut used = 0;
    text.graphemes(true)
        .take_while(|grapheme| {
            let next = used + cell_width(grapheme);
            if next > width {
                return false;
            }
            used = next;
            true
        })
        .collect()
}

#[cfg(test)]
#[path = "suggest_tests.rs"]
mod tests;
