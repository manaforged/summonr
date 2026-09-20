use sigil_tui::cell_width;

use super::app::App;
use super::canvas;
use super::model::Model;
use crate::tui::matching_commands;
use crate::{SlashCommand, parse_command};
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, Signal, card,
    container, signal,
};

use super::prelude::*;

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

    pub(super) fn open(self, draft: &str, caret: usize, model: &Model) -> bool {
        !self.hidden.get() && !self.rows(draft, caret, model).is_empty()
    }

    pub(super) fn hide(self, draft: &str, caret: usize, model: &Model) -> bool {
        if !self.open(draft, caret, model) {
            return false;
        }
        self.hidden.set(true);
        true
    }

    pub(super) fn shift(self, draft: &str, caret: usize, model: &Model, delta: isize) -> bool {
        let count = self.rows(draft, caret, model).len();
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

    pub(super) fn accept(self, draft: &str, caret: usize, model: &Model) -> Option<String> {
        if self.hidden.peek() {
            return None;
        }
        let rows = self.rows(draft, caret, model);
        let row = rows.get(self.cursor.peek().min(rows.len().saturating_sub(1)))?;
        (row.0 != draft).then(|| row.0.clone())
    }

    pub(super) fn complete(self, draft: &str, caret: usize, model: &Model) -> Option<String> {
        let rows = self.rows(draft, caret, model);
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

    fn rows(self, draft: &str, caret: usize, model: &Model) -> Vec<(String, String)> {
        if let Some((_, query)) = mention(draft, caret) {
            let query = query.to_lowercase();
            let mut peers: Vec<_> = model
                .peers
                .iter()
                .filter_map(|agent| {
                    let score = crate::tui::rank(&agent.id.to_lowercase(), "", &query)?;
                    Some((score, agent))
                })
                .collect();
            peers.sort_unstable_by_key(|(score, _)| *score);
            let mut matches: Vec<_> = model
                .files
                .iter()
                .filter_map(|path| {
                    let score = crate::tui::rank(&path.to_lowercase(), "", &query)?;
                    Some((score, path))
                })
                .collect();
            matches.sort_unstable();
            return peers
                .into_iter()
                .map(|(_, agent)| {
                    (
                        agent.id.clone(),
                        format!("{} · {} · {}", agent.kind, agent.model, agent.status),
                    )
                })
                .chain(
                    matches
                        .into_iter()
                        .map(|(_, path)| (path.clone(), String::new())),
                )
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
            .filter(|(name, _)| match parse_command(name) {
                Some(SlashCommand::PlanAccept | SlashCommand::PlanReject) => {
                    model.snapshot.pending_plan
                }
                Some(SlashCommand::Auto) => model.snapshot.plan,
                _ => true,
            })
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

pub(super) fn peers(text: &str, model: &Model) -> Vec<String> {
    let mut ids: Vec<String> = text
        .split_whitespace()
        .filter_map(|word| word.strip_prefix('@'))
        .filter(|id| model.peers.iter().any(|agent| agent.id == *id))
        .map(str::to_string)
        .collect();
    ids.dedup();
    ids
}

pub(super) fn reference(path: &str) -> String {
    format!("@{}", serde_json::Value::String(path.to_string()))
}

pub(super) fn widget(app: App, width: usize, full_width: usize) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_x(super::chat::margin(full_width) as f32 * canvas::cell().w)
                .shrink(0.0),
        )
        .children_reactive(move || {
            let _panes = app.panes.get();
            let draft = app.draft.get();
            let caret = app.caret.get();
            let rows = app.viewport.get().1;
            let model = app.model.borrow();
            if app.view_edit.get()
                || model.busy_since.is_some()
                || !app.commands.open(&draft, caret, &model)
                || width < 12
            {
                return Vec::new();
            }
            let matches = app.commands.rows(&draft, caret, &model);
            let cursor = app.commands.cursor(matches.len());
            let limit = matches
                .len()
                .min(LIMIT)
                .min(usize::from(rows / 2).saturating_sub(2));
            if limit == 0 {
                return Vec::new();
            }
            let start = cursor.saturating_add(1).saturating_sub(limit);
            let shown = &matches[start..matches.len().min(start + limit)];
            let popup = popup_width(&matches, width);
            let title = if mention(&draft, caret).is_some() {
                "mentions"
            } else {
                "commands"
            };
            let mut body = container()
                .layout(
                    LayoutStyle::flex_column()
                        .width(popup as f32 * canvas::cell().w)
                        .gap(2.0),
                )
                .child(group(format!(
                    "{title} {}–{}/{}",
                    start + 1,
                    start + shown.len(),
                    matches.len()
                )));
            for (offset, (name, description)) in shown.iter().enumerate() {
                let index = start + offset;
                let click = app.clone();
                body = body.child(
                    pick(index == cursor, false, name.clone(), description.clone()).pointer_down(
                        move |event| {
                            if event.button == Some(MouseButton::Left) {
                                click.commands.cursor.set(index);
                                click.accept();
                                event.stop_propagation();
                            }
                        },
                    ),
                );
            }
            let keys = if draft == "/" {
                "type to search · ↑↓ · tab · esc"
            } else {
                "↑↓ select · tab accept · esc"
            };
            vec![card(body.child(hint(keys, String::new())))]
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
