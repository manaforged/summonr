use std::borrow::Cow;

use serde::Serialize;
use serde_json::{Map, Value, json};

use super::super::traits::Thinking;
use super::super::{Wired, arguments};
use super::part::{Origin, Part, Use, control};
use crate::endpoint::CacheTtl;
use crate::provider::{CompletionRequest, Continuation, Message, Reasoning, Role, Step};

#[derive(Serialize)]
struct Turn<'a> {
    content: Vec<Part<'a>>,
    role: &'static str,
}

impl Turn<'_> {
    fn opens(&self, kind: &str) -> bool {
        self.content.first().and_then(Part::kind) == Some(kind)
    }
}

pub(crate) fn body(
    call: &CompletionRequest<'_>,
    reasoning: &Reasoning,
    ttl: CacheTtl,
    stream: bool,
) -> crate::error::Result<Wired> {
    let (mut system, mut turns) = turns(call)?;
    if let Some(last) = system.last_mut() {
        last["cache_control"] = control(ttl);
    }
    let mut tools = tools(call);
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = control(ttl);
    }
    if let Some(turn) = turns.last_mut()
        && let Some(last) = turn.content.iter_mut().rev().find(|part| !part.thought())
    {
        last.breakpoint(ttl);
    }
    let unthought = turns.len() > 1
        && turns[turns.len() - 1].opens("tool_result")
        && !turns[turns.len() - 2]
            .content
            .first()
            .is_some_and(Part::thought);
    let mut head = Map::new();
    head.insert("model".to_string(), json!(call.model));
    let ceiling = match call.traits.output {
        0 => 16_000,
        known => known,
    };
    let most = call.max_output_tokens.unwrap_or(ceiling);
    head.insert("max_tokens".to_string(), json!(most));
    if !system.is_empty() {
        head.insert("system".to_string(), Value::Array(system));
    }
    if !tools.is_empty() {
        head.insert("tools".to_string(), Value::Array(tools));
    }
    if stream {
        head.insert("stream".to_string(), json!(true));
    }
    think(&mut head, call, reasoning, most, unthought);
    Ok(Wired {
        head: Value::Object(head),
        messages: Some(serde_json::to_vec(&turns)?),
        slot: super::super::Slot::Messages,
    })
}

fn turns<'a>(call: &CompletionRequest<'a>) -> crate::error::Result<(Vec<Value>, Vec<Turn<'a>>)> {
    let mut system = Vec::new();
    let mut turns: Vec<Turn<'a>> = Vec::new();
    for message in call.messages {
        match message.role {
            Role::System if turns.is_empty() => {
                system.push(json!({"type": "text", "text": message.content}));
            }
            Role::System => push_user(
                &mut turns,
                vec![json!({"type": "text", "text": message.content})],
            ),
            Role::Tool => push_result(&mut turns, message),
            Role::User => push_user(&mut turns, user_parts(message)),
            Role::Assistant => {
                let content = assistant_parts(message, &call.model)?;
                if !content.is_empty() {
                    turns.push(Turn {
                        content,
                        role: "assistant",
                    });
                }
            }
        }
    }
    Ok((system, turns))
}

fn think(
    body: &mut Map<String, Value>,
    call: &CompletionRequest<'_>,
    reasoning: &Reasoning,
    most: u32,
    unthought: bool,
) {
    let plan = reasoning.plan(call.traits);
    let thinking = call.traits.thinking;
    match plan.step(call.effort) {
        Step::Default if call.traits.bound => {
            body.insert("thinking".to_string(), adaptive(call));
        }
        Step::Default => {}
        Step::Off(off) => {
            if let Some((key, value)) = off.fragment() {
                body.insert(key.to_string(), value);
            }
        }
        Step::Level(level) => {
            if matches!(thinking, Thinking::Adaptive | Thinking::EffortBudget) {
                body.insert(
                    "output_config".to_string(),
                    json!({"effort": level.as_str()}),
                );
            }
            if unthought && thinking != Thinking::Adaptive {
                return;
            }
            let budget = plan.budget(level, most);
            match thinking {
                Thinking::Adaptive => {
                    body.insert("thinking".to_string(), adaptive(call));
                }
                Thinking::Budgeted | Thinking::EffortBudget if (1024..most).contains(&budget) => {
                    body.insert(
                        "thinking".to_string(),
                        json!({"type": "enabled", "budget_tokens": budget}),
                    );
                }
                Thinking::Budgeted
                | Thinking::EffortBudget
                | Thinking::Budget
                | Thinking::Level
                | Thinking::None => {}
            }
        }
    }
}

fn adaptive(call: &CompletionRequest<'_>) -> Value {
    if call.traits.bound {
        json!({
            "type": "adaptive",
            "display": "summarized",
            "block_binding": {"prefix_mismatch_behavior": "drop_block"},
        })
    } else {
        json!({"type": "adaptive"})
    }
}

fn tools(call: &CompletionRequest<'_>) -> Vec<Value> {
    call.tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.function.name,
                "description": tool.function.description,
                "input_schema": tool.function.parameters,
            })
        })
        .collect()
}

fn user_parts(message: &Message) -> Vec<Value> {
    let mut parts: Vec<Value> = message
        .images
        .iter()
        .map(|image| {
            json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.media_type,
                    "data": image.data,
                },
            })
        })
        .collect();
    if !message.content.is_empty() {
        parts.push(json!({"type": "text", "text": message.content}));
    }
    parts
}

fn assistant_parts<'a>(message: &'a Message, model: &str) -> crate::error::Result<Vec<Part<'a>>> {
    if let Some(kept) = retained(message, model)? {
        return Ok(kept);
    }
    let mut parts = Vec::new();
    if !message.content.is_empty() {
        parts.push(Part::owned(
            json!({"type": "text", "text": message.content}),
        ));
    }
    for call in &message.tool_calls {
        parts.push(Part::Use(Use {
            origin: Origin::Fresh(&call.id),
            name: &call.function.name,
            input: arguments(&call.function.name, &call.function.arguments)?,
            cached: None,
        }));
    }
    Ok(parts)
}

fn retained<'a>(message: &'a Message, model: &str) -> crate::error::Result<Option<Vec<Part<'a>>>> {
    let Some(blocks) = message.tool_calls.iter().find_map(|call| {
        if let Some(Continuation::Anthropic {
            model: made,
            blocks,
        }) = call.continuation.as_ref()
            && made == model
        {
            Some(blocks)
        } else {
            None
        }
    }) else {
        return Ok(None);
    };
    let mut calls = message.tool_calls.iter();
    let mut parts = Vec::with_capacity(blocks.len());
    for block in blocks {
        let Some(kept) = block.as_object().filter(|_| block["type"] == "tool_use") else {
            parts.push(Part::Value(Cow::Borrowed(block)));
            continue;
        };
        let Some(call) = calls.next().filter(|call| block["id"] == call.id.as_str()) else {
            return Ok(None);
        };
        parts.push(Part::Use(Use {
            origin: Origin::Kept(kept),
            name: &call.function.name,
            input: arguments(&call.function.name, &call.function.arguments)?,
            cached: None,
        }));
    }
    Ok(calls.next().is_none().then_some(parts))
}

fn push_result(turns: &mut Vec<Turn<'_>>, message: &Message) {
    let mut block = json!({
        "type": "tool_result",
        "tool_use_id": message.tool_call_id.as_deref().unwrap_or_default(),
        "content": user_parts(message),
    });
    if message.failed {
        block["is_error"] = json!(true);
    }
    let paired = turns.len() > 1 && turns[turns.len() - 2].role == "assistant";
    let merged = turns.last().is_some_and(|turn| turn.opens("tool_result"));
    if (paired || merged)
        && let Some(last) = turns.last_mut()
        && last.role == "user"
    {
        let at = last
            .content
            .iter()
            .take_while(|part| part.kind() == Some("tool_result"))
            .count();
        last.content.insert(at, Part::owned(block));
        return;
    }
    turns.push(Turn {
        content: vec![Part::owned(block)],
        role: "user",
    });
}

fn push_user(turns: &mut Vec<Turn<'_>>, added: Vec<Value>) {
    if added.is_empty() {
        return;
    }
    let added = added.into_iter().map(Part::owned);
    if let Some(last) = turns.last_mut()
        && last.role == "user"
    {
        last.content.extend(added);
        return;
    }
    turns.push(Turn {
        content: added.collect(),
        role: "user",
    });
}
