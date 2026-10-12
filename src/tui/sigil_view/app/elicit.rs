use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use crate::mcp::{Choice, Field, FieldKind, Form, FormAction, FormAnswer};

use super::*;
use crate::prefs::Hotkeys;
use crate::tui::sigil_view::ui::{Hints, Key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Stage {
    Filling,
    Review,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::tui::sigil_view) struct FormState {
    form: Form,
    index: usize,
    values: Map<String, Value>,
    error: Option<String>,
    cursor: usize,
    picked: BTreeSet<usize>,
    pub(in crate::tui::sigil_view) stage: Stage,
    dialog: u64,
    saved: (String, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Reply {
    Send,
    Decline,
    Edit,
    Cancel,
}

impl FormState {
    fn new(form: Form, dialog: u64, saved: (String, usize)) -> Self {
        let mut state = Self {
            form,
            index: 0,
            values: Map::new(),
            error: None,
            cursor: 0,
            picked: BTreeSet::new(),
            stage: Stage::Filling,
            dialog,
            saved,
        };
        state.focus();
        state
    }

    fn field(&self) -> Option<&Field> {
        self.form.fields.get(self.index)
    }

    fn current(&self) -> Option<&Value> {
        self.field()
            .and_then(|field| self.values.get(&field.name).or(field.default.as_ref()))
    }

    fn options(&self) -> Option<(&[Choice], bool)> {
        match &self.field()?.kind {
            FieldKind::Choice { options, .. } => Some((options, false)),
            FieldKind::Choices { options, .. } => Some((options, true)),
            FieldKind::Text { .. } | FieldKind::Number { .. } | FieldKind::Boolean => None,
        }
    }

    fn focus(&mut self) {
        let chosen: BTreeSet<String> = match self.current() {
            Some(Value::String(text)) => BTreeSet::from([text.clone()]),
            Some(Value::Array(items)) => items.iter().map(Field::shown).collect(),
            _other => BTreeSet::new(),
        };
        let found: BTreeSet<usize> = self.options().map_or_else(BTreeSet::new, |(options, _)| {
            (0..options.len())
                .filter(|index| chosen.contains(&options[*index].value))
                .collect()
        });
        self.cursor = found.first().copied().unwrap_or(0);
        self.picked = if self.options().is_some_and(|(_, many)| many) {
            found
        } else {
            BTreeSet::new()
        };
    }

    fn prefill(&self) -> String {
        match self.options() {
            Some((options, _)) => match self.current() {
                Some(Value::String(text)) if !options.iter().any(|o| &o.value == text) => {
                    text.clone()
                }
                _other => String::new(),
            },
            None => self.current().map(Field::shown).unwrap_or_default(),
        }
    }

    fn highlighted(&self) -> Option<Value> {
        let (options, many) = self.options()?;
        if !many {
            return options.get(self.cursor).map(|option| json!(option.value));
        }
        if self.picked.is_empty() {
            return None;
        }
        Some(json!(
            self.picked
                .iter()
                .filter_map(|index| options.get(*index).map(|option| option.value.clone()))
                .collect::<Vec<_>>()
        ))
    }

    pub(in crate::tui::sigil_view) fn shown(&self, typed: &str) -> Vec<String> {
        let typed = typed.trim();
        let head = match &self.form.server {
            Some(server) => format!("mcp {server} asks: {}", self.form.message),
            None => "The model asks".to_string(),
        };
        let mut lines = vec![head];
        match self.stage {
            Stage::Filling => self.filling_lines(&mut lines, typed),
            Stage::Review => self.review_lines(&mut lines),
        }
        lines
    }

    fn filling_lines(&self, lines: &mut Vec<String>, typed: &str) {
        if let Some(field) = self.field() {
            lines.push(format!(
                "{}/{} {}",
                self.index + 1,
                self.form.fields.len(),
                field.title
            ));
            if let Some(description) = &field.description {
                lines.push(description.clone());
            }
            if let Some((options, many)) = self.options() {
                lines.extend(options.iter().enumerate().map(|(index, option)| {
                    let mark = match (many, self.picked.contains(&index)) {
                        (false, _) => format!("{}.", index + 1),
                        (true, true) => "[x]".to_string(),
                        (true, false) => "[ ]".to_string(),
                    };
                    let cursor = if index == self.cursor && typed.is_empty() {
                        "❯"
                    } else {
                        " "
                    };
                    let about = option
                        .description
                        .as_ref()
                        .map_or_else(String::new, |about| format!("  {about}"));
                    format!("{cursor} {mark} {}{about}", option.title)
                }));
            }
            if !typed.is_empty() {
                lines.push(format!("❯ {typed}"));
            }
            lines.push(facts(field));
        }
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
    }

    fn review_lines(&self, lines: &mut Vec<String>) {
        for field in &self.form.fields {
            let value = self
                .values
                .get(&field.name)
                .map_or_else(String::new, Field::shown);
            lines.push(format!("{}: {value}", field.title));
        }
    }
}

fn facts(field: &Field) -> String {
    let mut parts = vec![hint(&field.kind)];
    if field.required {
        parts.push("required".to_string());
    }
    if let Some(default) = &field.default {
        parts.push(format!("default {}", Field::shown(default)));
    }
    parts.retain(|part| !part.is_empty());
    parts.join(" · ")
}

fn hint(kind: &FieldKind) -> String {
    match kind {
        FieldKind::Text { format, .. } => {
            format.map_or_else(String::new, |format| format.label().to_string())
        }
        FieldKind::Number {
            integer,
            minimum,
            maximum,
        } => number_hint(*integer, *minimum, *maximum),
        FieldKind::Boolean => "yes or no".to_string(),
        FieldKind::Choice { custom, .. } => picking(
            &Hints::new()
                .key(Key::UpDown, "move")
                .key(Key::Enter, "picks"),
            *custom,
        ),
        FieldKind::Choices { custom, .. } => picking(
            &Hints::new()
                .key(Key::UpDown, "move")
                .key(Key::Space, "toggles")
                .key(Key::Enter, "confirms"),
            *custom,
        ),
    }
}

fn picking(hints: &Hints, custom: bool) -> String {
    let keys = hints.text(&Hotkeys::default());
    if custom {
        format!("{keys} · or type your own answer")
    } else {
        keys
    }
}

fn number_hint(integer: bool, minimum: Option<f64>, maximum: Option<f64>) -> String {
    let noun = if integer { "whole number" } else { "number" };
    match (minimum, maximum) {
        (Some(low), Some(high)) => format!("{noun} {low} to {high}"),
        (Some(low), None) => format!("{noun} from {low}"),
        (None, Some(high)) => format!("{noun} up to {high}"),
        (None, None) => noun.to_string(),
    }
}

impl App {
    pub(in crate::tui::sigil_view) fn form_open(&self, form: Option<Form>, dialog: u64) {
        let Some(form) = form else {
            self.form_close();
            return;
        };
        let saved = self.form.peek().map_or_else(
            || (self.draft.peek(), self.caret.peek()),
            |state| state.saved,
        );
        let state = FormState::new(form, dialog, saved);
        let draft = state.prefill();
        self.form.set(Some(state));
        self.replace(draft);
    }

    pub(in crate::tui::sigil_view) fn form_close(&self) {
        let Some(state) = self.form.peek() else {
            return;
        };
        self.form.set(None);
        let (draft, caret) = state.saved;
        self.replace(draft);
        self.caret.set(caret);
    }

    pub(in crate::tui::sigil_view) fn form_stage(&self) -> Option<Stage> {
        self.form.peek().map(|state| state.stage)
    }

    pub(in crate::tui::sigil_view) fn form_enter(&self) -> bool {
        let Some(mut state) = self.form.peek() else {
            return false;
        };
        if state.stage == Stage::Review {
            return true;
        }
        let Some(field) = state.field().cloned() else {
            return true;
        };
        let typed = self.draft.peek();
        let read = match state.highlighted().filter(|_| typed.trim().is_empty()) {
            Some(value) => field.check(&value).map(|()| Some(value)),
            None => field.read(&typed),
        };
        match read {
            Err(error) => state.error = Some(error),
            Ok(value) => {
                match value {
                    Some(value) => drop(state.values.insert(field.name.clone(), value)),
                    None => drop(state.values.remove(&field.name)),
                }
                state.error = None;
                state.index += 1;
                if state.index >= state.form.fields.len() {
                    state.stage = Stage::Review;
                }
                state.focus();
                self.replace(state.prefill());
            }
        }
        let direct = state.stage == Stage::Review
            && state.form.server.is_none()
            && state.form.fields.len() == 1;
        self.form.set(Some(state));
        if direct {
            self.form_reply(Reply::Send);
        }
        true
    }

    pub(in crate::tui::sigil_view) fn form_move(&self, dir: isize) -> bool {
        let Some(mut state) = self
            .form
            .peek()
            .filter(|state| state.stage == Stage::Filling)
        else {
            return false;
        };
        let Some(count) = state.options().map(|(options, _)| options.len()) else {
            return false;
        };
        let count = isize::try_from(count).unwrap_or(isize::MAX).max(1);
        let at = isize::try_from(state.cursor).unwrap_or(0);
        state.cursor = usize::try_from((at + dir).rem_euclid(count)).unwrap_or(0);
        self.form.set(Some(state));
        true
    }

    pub(in crate::tui::sigil_view) fn form_toggle(&self) -> bool {
        let Some(mut state) = self
            .form
            .peek()
            .filter(|state| state.stage == Stage::Filling)
        else {
            return false;
        };
        if !self.draft.peek().is_empty() || !state.options().is_some_and(|(_, many)| many) {
            return false;
        }
        let cursor = state.cursor;
        if !state.picked.remove(&cursor) {
            state.picked.insert(cursor);
        }
        self.form.set(Some(state));
        true
    }

    pub(in crate::tui::sigil_view) fn form_reply(&self, reply: Reply) {
        let Some(mut state) = self.form.peek() else {
            return;
        };
        let answer = match reply {
            Reply::Send => FormAnswer {
                action: FormAction::Accept,
                content: Some(state.values.clone()),
            },
            Reply::Decline => FormAnswer::declined(),
            Reply::Cancel => FormAnswer::cancelled(),
            Reply::Edit => {
                state.index = 0;
                state.error = None;
                state.stage = Stage::Filling;
                state.focus();
                let draft = state.prefill();
                self.form.set(Some(state));
                self.replace(draft);
                return;
            }
        };
        self.form_close();
        match serde_json::to_string(&answer) {
            Ok(text) => self.steer_answer(state.dialog, text),
            Err(error) => self.push_entry(Entry::Error(format!("mcp form: {error}"))),
        }
    }
}
