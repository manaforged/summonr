use serde::Deserialize;
use serde_json::json;

use super::*;
use crate::mcp::{Choice, Field, FieldKind, Form, FormAction, FormAnswer};
use crate::provider::ToolDefinition;

pub(crate) const ASK: &str = "ask";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    questions: Vec<Question>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    question: String,
    #[serde(default)]
    options: Vec<Offer>,
    #[serde(default)]
    multiple: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Offer {
    label: String,
    #[serde(default)]
    description: Option<String>,
}

pub(in crate::runtime) fn definition() -> ToolDefinition {
    crate::tools::tool(
        ASK,
        "Ask the user one to four questions and wait for the answers. Use it only for a choice that is the user's to make and that you cannot settle from the code, the docs, or a command. Each option is a complete answer of one to five words with a one-sentence description; put the option you recommend first. The user can always type their own answer, so never add an Other option. Leave options empty for a free-text question; set multiple to accept several options.",
        json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 4,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {"type": "string"},
                            "options": {
                                "type": "array",
                                "maxItems": 4,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string"},
                                        "description": {"type": "string"}
                                    },
                                    "required": ["label"],
                                    "additionalProperties": false
                                }
                            },
                            "multiple": {"type": "boolean"}
                        },
                        "required": ["question"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["questions"],
            "additionalProperties": false
        }),
    )
}

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) fn ask_available(&self) -> bool {
        self.depth == 0 && self.steer_shared.is_some()
    }

    pub(super) async fn ask(
        &mut self,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Result<ToolOutput>> {
        let args = match serde_json::from_str::<Args>(&call.function.arguments) {
            Ok(args) if !args.questions.is_empty() => args,
            Ok(_) => return Ok(Err(Error::Tool("ask needs a question".to_string()))),
            Err(error) => {
                return Ok(Err(Error::Tool(format!(
                    "ask arguments are invalid: {error}"
                ))));
            }
        };
        let form = form(args.questions);
        let answer = self.elicit(form.clone(), events, emit).await?;
        Ok(Ok(ToolOutput::ok(reply(&form, &answer))))
    }
}

fn form(questions: Vec<Question>) -> Form {
    let fields = questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| Field {
            name: (index + 1).to_string(),
            title: question.question,
            description: None,
            kind: kind(question.options, question.multiple),
            required: false,
            default: None,
        })
        .collect::<Vec<_>>();
    Form {
        server: None,
        message: fields
            .first()
            .map(|field| field.title.clone())
            .unwrap_or_default(),
        fields,
    }
}

fn kind(options: Vec<Offer>, multiple: bool) -> FieldKind {
    let options = options
        .into_iter()
        .map(|offer| Choice {
            value: offer.label.clone(),
            title: offer.label,
            description: offer.description,
        })
        .collect::<Vec<_>>();
    match (options.is_empty(), multiple) {
        (true, _) => FieldKind::Text {
            format: None,
            min_length: None,
            max_length: None,
            pattern: None,
        },
        (false, false) => FieldKind::Choice {
            options,
            custom: true,
        },
        (false, true) => FieldKind::Choices {
            options,
            min_items: None,
            max_items: None,
            custom: true,
        },
    }
}

fn reply(form: &Form, answer: &FormAnswer) -> String {
    let content = match (&answer.action, &answer.content) {
        (FormAction::Accept, Some(content)) => content,
        _dismissed => return "The user dismissed the question without answering.".to_string(),
    };
    form.fields
        .iter()
        .map(|field| {
            let given = content
                .get(&field.name)
                .map_or_else(|| "(no answer)".to_string(), Field::shown);
            format!("{} → {given}", field.title)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "ask_tests.rs"]
mod tests;
