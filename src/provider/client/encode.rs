use super::*;

pub(super) fn wire_messages(messages: &[Message], keep_reasoning: bool) -> Result<Vec<Value>> {
    let mut prefix = String::new();
    let mut rest = Vec::new();
    for message in messages {
        if message.role == Role::System && rest.is_empty() {
            if !prefix.is_empty() {
                prefix.push_str("\n\n");
            }
            prefix.push_str(&message.content);
            continue;
        }
        if message.role == Role::System {
            rest.push(message_value(
                &Message::user(message.content.clone()),
                keep_reasoning,
            )?);
        } else {
            rest.push(message_value(message, keep_reasoning)?);
        }
    }
    let mut out = Vec::new();
    if !prefix.is_empty() {
        out.push(message_value(&Message::system(prefix), keep_reasoning)?);
    }
    out.extend(rest);
    Ok(out)
}

pub(super) fn message_value(message: &Message, keep_reasoning: bool) -> Result<Value> {
    let mut value = serde_json::to_value(message)?;
    if message.role == Role::Assistant
        && let Some(object) = value.as_object_mut()
    {
        if keep_reasoning {
            object
                .entry("reasoning_content")
                .or_insert_with(|| json!(""));
        } else {
            object.remove("reasoning_content");
        }
    }
    if let Some(calls) = value.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for call in calls {
            if let Some(call) = call.as_object_mut() {
                call.remove("continuation");
                if let Some(function) = call.get_mut("function").and_then(Value::as_object_mut)
                    && let Some(arguments) = function.get("arguments").and_then(Value::as_str)
                    && serde_json::from_str::<Value>(arguments).is_err()
                {
                    function.insert("arguments".to_string(), json!("{}"));
                }
            }
        }
    }
    if message.images.is_empty() {
        return Ok(value);
    }
    let mut parts = Vec::with_capacity(message.images.len() + 1);
    if !message.content.is_empty() {
        parts.push(json!({ "type": "text", "text": message.content }));
    }
    parts.extend(message.images.iter().map(|image| {
        json!({
            "type": "image_url",
            "image_url": { "url": image.url() }
        })
    }));
    value["content"] = Value::Array(parts);
    Ok(value)
}
