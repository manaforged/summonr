use serde::ser::{SerializeMap, SerializeSeq, Serializer};

use super::*;

pub(crate) struct Body<'a> {
    head: serde_json::Map<String, Value>,
    slot: crate::provider::wire::Slot,
    messages: Option<Vec<u8>>,
    tools: Option<&'a [u8]>,
}

impl<'a> Body<'a> {
    pub(super) fn new(
        head: Value,
        slot: crate::provider::wire::Slot,
        messages: Option<Vec<u8>>,
        tools: Option<&'a [u8]>,
    ) -> Self {
        let Value::Object(mut head) = head else {
            return Self {
                head: serde_json::Map::new(),
                slot,
                messages,
                tools: None,
            };
        };
        if messages.is_some() {
            head.insert(slot.key().to_string(), Value::Null);
        }
        let tools = tools.filter(|_| head.contains_key("tools"));
        Self {
            head,
            slot,
            messages,
            tools,
        }
    }

    pub(super) fn set(&mut self, key: &str, value: Value) {
        if key == self.slot.key() {
            self.messages = None;
        }
        if key == "tools" {
            self.tools = None;
        }
        self.head.insert(key.to_string(), value);
    }

    pub(crate) fn model(&self) -> Option<&str> {
        self.head.get("model").and_then(Value::as_str)
    }

    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(4096);
        out.push(b'{');
        for (index, (key, value)) in self.head.iter().enumerate() {
            if index > 0 {
                out.push(b',');
            }
            serde_json::to_writer(&mut out, key)?;
            out.push(b':');
            let raw = if key == self.slot.key() {
                self.messages.as_deref()
            } else if key == "tools" {
                self.tools
            } else {
                None
            };
            if let Some(raw) = raw {
                out.extend_from_slice(raw);
            } else {
                serde_json::to_writer(&mut out, value)?;
            }
        }
        out.push(b'}');
        Ok(out)
    }
}

pub(crate) struct Messages<'a> {
    pub(super) list: &'a [Message],
    pub(super) keep_reasoning: bool,
}

impl Serialize for Messages<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let (lead, prefix) = system_prefix(self.list);
        let mut seq = serializer.serialize_seq(None)?;
        if !prefix.is_empty() {
            seq.serialize_element(&View::text(Role::System, &prefix, &[]))?;
        }
        let mut attached: Vec<&Image> = Vec::new();
        for message in &self.list[lead..] {
            element(&mut seq, message, &mut attached, self.keep_reasoning)?;
        }
        if !attached.is_empty() {
            seq.serialize_element(&View::attachments(&attached))?;
        }
        seq.end()
    }
}

fn system_prefix(list: &[Message]) -> (usize, String) {
    let lead = list
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    let mut prefix = String::new();
    for message in &list[..lead] {
        if !prefix.is_empty() {
            prefix.push_str("\n\n");
        }
        prefix.push_str(&message.content);
    }
    (lead, prefix)
}

fn element<'a, Q: SerializeSeq>(
    seq: &mut Q,
    message: &'a Message,
    attached: &mut Vec<&'a Image>,
    keep: bool,
) -> std::result::Result<(), Q::Error> {
    if message.role != Role::Tool && !attached.is_empty() {
        seq.serialize_element(&View::attachments(attached))?;
        attached.clear();
    }
    if message.role == Role::Tool && !message.images.is_empty() {
        attached.extend(&message.images);
        return seq.serialize_element(&View::of(message, &[], keep));
    }
    if message.role == Role::System {
        return seq.serialize_element(&View::text(Role::User, &message.content, &[]));
    }
    let images: Vec<&Image> = message.images.iter().collect();
    seq.serialize_element(&View::of(message, &images, keep))
}

struct View<'a, 'b> {
    role: Role,
    content: &'a str,
    images: &'b [&'a Image],
    tool_calls: &'a [ToolCall],
    tool_call_id: Option<&'a str>,
    reasoning: Option<&'a str>,
    keep_reasoning: bool,
}

impl<'a, 'b> View<'a, 'b> {
    fn text(role: Role, content: &'a str, images: &'b [&'a Image]) -> Self {
        Self {
            role,
            content,
            images,
            tool_calls: &[],
            tool_call_id: None,
            reasoning: None,
            keep_reasoning: false,
        }
    }

    fn attachments(images: &'b [&'a Image]) -> Self {
        Self::text(Role::User, "Attached image(s) from tool result:", images)
    }

    fn of(message: &'a Message, images: &'b [&'a Image], keep_reasoning: bool) -> Self {
        Self {
            role: message.role,
            content: &message.content,
            images,
            tool_calls: &message.tool_calls,
            tool_call_id: message.tool_call_id.as_deref(),
            reasoning: message.reasoning.as_deref(),
            keep_reasoning,
        }
    }
}

impl Serialize for View<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("role", &self.role)?;
        if self.images.is_empty() {
            map.serialize_entry("content", self.content)?;
        } else {
            map.serialize_entry("content", &Parts(self.content, self.images))?;
        }
        if !self.tool_calls.is_empty() {
            map.serialize_entry("tool_calls", &Calls(self.tool_calls))?;
        }
        if let Some(id) = self.tool_call_id {
            map.serialize_entry("tool_call_id", id)?;
        }
        match (self.role, self.keep_reasoning, self.reasoning) {
            (Role::Assistant, true, reasoning) => {
                map.serialize_entry("reasoning_content", reasoning.unwrap_or_default())?;
            }
            (Role::Assistant, false, _) | (_, _, None) => {}
            (_, _, Some(reasoning)) => map.serialize_entry("reasoning_content", reasoning)?,
        }
        map.end()
    }
}

struct Parts<'a, 'b>(&'a str, &'b [&'a Image]);

impl Serialize for Parts<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(None)?;
        if !self.0.is_empty() {
            seq.serialize_element(&json!({ "type": "text", "text": self.0 }))?;
        }
        for image in self.1 {
            seq.serialize_element(&json!({
                "type": "image_url",
                "image_url": { "url": image.url() }
            }))?;
        }
        seq.end()
    }
}

struct Calls<'a>(&'a [ToolCall]);

impl Serialize for Calls<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for call in self.0 {
            seq.serialize_element(&Call(call))?;
        }
        seq.end()
    }
}

struct Call<'a>(&'a ToolCall);

impl Serialize for Call<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let call = self.0;
        let arguments = if call.function.valid() {
            call.function.arguments.as_str()
        } else {
            "{}"
        };
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &call.id)?;
        map.serialize_entry("type", &call.kind)?;
        map.serialize_entry(
            "function",
            &Function {
                name: &call.function.name,
                arguments,
            },
        )?;
        map.end()
    }
}

#[derive(Serialize)]
struct Function<'a> {
    name: &'a str,
    arguments: &'a str,
}
