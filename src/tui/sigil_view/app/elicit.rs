use serde_json::{Map, Value};

use crate::mcp::{Choice, Field, FieldKind, Form, FormAction, FormAnswer};

use super::*;

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
    pub(in crate::tui::sigil_view) stage: Stage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Reply {
    Send,
    Decline,
    Edit,
    Cancel,
}

impl FormState {
    fn new(form: Form) -> Self {
        Self {
            form,
            index: 0,
            values: Map::new(),
            error: None,
            stage: Stage::Filling,
        }
    }

    fn field(&self) -> Option<&Field> {
        self.form.fields.get(self.index)
    }

    fn prefill(&self) -> String {
        self.field()
            .and_then(|field| self.values.get(&field.name).or(field.default.as_ref()))
            .map(shown)
            .unwrap_or_default()
    }

    pub(in crate::tui::sigil_view) fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "mcp {} asks: {}",
            self.form.server, self.form.message
        )];
        match self.stage {
            Stage::Filling => self.filling_lines(&mut lines),
            Stage::Review => self.review_lines(&mut lines),
        }
        lines
    }

    fn filling_lines(&self, lines: &mut Vec<String>) {
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
            lines.push(facts(field));
        }
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
    }

    fn review_lines(&self, lines: &mut Vec<String>) {
        for field in &self.form.fields {
            let value = self.values.get(&field.name).map_or_else(String::new, shown);
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
        parts.push(format!("default {}", shown(default)));
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
        FieldKind::Choice { options } => numbered(options),
        FieldKind::Choices { options, .. } => format!("{} · comma-separated", numbered(options)),
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

fn numbered(options: &[Choice]) -> String {
    options
        .iter()
        .enumerate()
        .map(|(index, option)| format!("{} {}", index + 1, option.title))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn shown(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "yes".to_string(),
        Value::Bool(false) => "no".to_string(),
        Value::Array(items) => items.iter().map(shown).collect::<Vec<_>>().join(", "),
        other @ (Value::Null | Value::Number(_) | Value::Object(_)) => other.to_string(),
    }
}

impl App {
    pub(in crate::tui::sigil_view) fn form_open(&self, form: Option<Form>) {
        let state = form.map(FormState::new);
        let draft = state.as_ref().map(FormState::prefill);
        self.form.set(state);
        if let Some(draft) = draft {
            self.replace(draft);
        }
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
        match field.read(&self.draft.peek()) {
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
                self.replace(state.prefill());
            }
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
                let draft = state.prefill();
                self.form.set(Some(state));
                self.replace(draft);
                return;
            }
        };
        self.form.set(None);
        self.replace(String::new());
        match serde_json::to_string(&answer) {
            Ok(text) => self.steer_answer(text),
            Err(error) => self.push_entry(Entry::Error(format!("mcp form: {error}"))),
        }
    }
}
