use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use summonr_agent::mcp::{Choice, Field, FieldKind, Form, FormAction, FormAnswer};

use super::*;

mod hints;
use hints::facts;

const TYPED_BELOW: &str = "your typed answer below replaces the choices · ↵ sends it";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Filling,
    Review,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FormState {
    form: Form,
    index: usize,
    values: Map<String, Value>,
    error: Option<String>,
    cursor: usize,
    picked: BTreeSet<usize>,
    pub(crate) stage: Stage,
    dialog: u64,
    saved: (String, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Send,
    Decline,
    Edit,
    Cancel,
}

impl FormState {
    fn new(form: Form, dialog: u64, saved: (String, usize)) -> Self {
        let stage = if form.fields.is_empty() {
            Stage::Review
        } else {
            Stage::Filling
        };
        let mut state = Self {
            form,
            index: 0,
            values: Map::new(),
            error: None,
            cursor: 0,
            picked: BTreeSet::new(),
            stage,
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

    pub(crate) fn choice_titles(&self) -> Vec<String> {
        let Some((options, many)) = self.options() else {
            return Vec::new();
        };
        options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                if many && self.picked.contains(&index) {
                    format!("✓ {}", option.title)
                } else {
                    option.title.clone()
                }
            })
            .collect()
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

    pub(crate) fn shown(&self, typed: &str) -> Vec<String> {
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
                lines.push(TYPED_BELOW.to_string());
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

impl App {
    pub(crate) fn form_open(&self, form: Option<Form>, dialog: u64) {
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

    pub(crate) fn form_close(&self) {
        let Some(state) = self.form.peek() else {
            return;
        };
        self.form.set(None);
        let (draft, caret) = state.saved;
        self.replace(draft);
        self.caret.set(caret);
    }

    pub(crate) fn form_hidden(&self) -> bool {
        self.form.peek().is_some() && self.approval.peek().is_some()
    }

    fn form_shown(&self) -> Option<FormState> {
        self.form.peek().filter(|_| !self.form_hidden())
    }

    pub(crate) fn form_stage(&self) -> Option<Stage> {
        self.form_shown().map(|state| state.stage)
    }

    pub(crate) fn form_enter(&self) -> bool {
        let Some(mut state) = self.form_shown() else {
            return false;
        };
        if state.stage == Stage::Review {
            self.form_reply(Reply::Send);
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

    fn form_filling(&self) -> Option<FormState> {
        self.form_shown()
            .filter(|state| state.stage == Stage::Filling)
    }

    pub(crate) fn form_move(&self, dir: isize) -> bool {
        let Some(mut state) = self.form_filling() else {
            return false;
        };
        let Some(count) = state.options().map(|(options, _)| options.len()) else {
            return false;
        };
        state.cursor = crate::ui::cycle(state.cursor, dir, count);
        self.form.set(Some(state));
        true
    }

    pub(crate) fn form_toggle(&self) -> bool {
        let Some(mut state) = self.form_filling() else {
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

    pub(crate) fn form_pick(&self, index: usize) {
        let Some(mut state) = self.form_shown() else {
            return;
        };
        let many = state.options().is_some_and(|(_, many)| many);
        state.cursor = index;
        self.form.set(Some(state));
        if many {
            self.form_toggle();
        } else {
            self.replace(String::new());
            self.form_enter();
        }
    }

    pub(crate) fn form_reply(&self, reply: Reply) {
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
            Reply::Edit if state.form.fields.is_empty() => return,
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
