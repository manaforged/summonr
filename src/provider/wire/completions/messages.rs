use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};
use serde_json::json;

use crate::provider::{Image, Message, Role, ToolCall};

pub(crate) struct Messages<'a> {
    pub(crate) list: &'a [Message],
    pub(crate) keep_reasoning: bool,
    pub(crate) cached: bool,
    pub(crate) assistant_after_tool: bool,
}

impl Serialize for Messages<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let (lead, prefix) = system_prefix(self.list);
        let mut seq = Turns {
            seq: serializer.serialize_seq(None)?,
            after_tool: self.assistant_after_tool,
            last: None,
        };
        if !prefix.is_empty() {
            let mut system = View::text(Role::System, &prefix, &[]);
            system.cached = self.cached;
            seq.push(&system)?;
        }
        let trailing = self
            .list
            .iter()
            .rev()
            .take_while(|message| message.role == Role::Tool)
            .any(|message| !message.images.is_empty());
        let last = self.list.len().saturating_sub(1).max(lead);
        let mut attached: Vec<&Image> = Vec::new();
        for (index, message) in self.list.iter().enumerate().skip(lead) {
            let cached = self.cached && !trailing && index == last;
            element(
                &mut seq,
                message,
                &mut attached,
                self.keep_reasoning,
                cached,
            )?;
        }
        if !attached.is_empty() {
            let mut view = View::attachments(&attached);
            view.cached = self.cached;
            seq.push(&view)?;
        }
        seq.seq.end()
    }
}

struct Turns<Q> {
    seq: Q,
    after_tool: bool,
    last: Option<Role>,
}

impl<Q: SerializeSeq> Turns<Q> {
    fn push(&mut self, view: &View<'_, '_>) -> std::result::Result<(), Q::Error> {
        if self.after_tool && self.last == Some(Role::Tool) && view.role == Role::User {
            self.seq
                .serialize_element(&View::text(Role::Assistant, " ", &[]))?;
        }
        self.last = Some(view.role);
        self.seq.serialize_element(view)
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
    seq: &mut Turns<Q>,
    message: &'a Message,
    attached: &mut Vec<&'a Image>,
    keep: bool,
    cached: bool,
) -> std::result::Result<(), Q::Error> {
    if message.role != Role::Tool && !attached.is_empty() {
        seq.push(&View::attachments(attached))?;
        attached.clear();
    }
    if message.role == Role::Tool && !message.images.is_empty() {
        attached.extend(&message.images);
        return seq.push(&View::of(message, &[], keep));
    }
    if message.role == Role::System {
        let mut view = View::text(Role::User, &message.content, &[]);
        view.cached = cached;
        return seq.push(&view);
    }
    let images: Vec<&Image> = message.images.iter().collect();
    let mut view = View::of(message, &images, keep);
    view.cached = cached;
    seq.push(&view)
}

struct View<'a, 'b> {
    role: Role,
    content: &'a str,
    images: &'b [&'a Image],
    tool_calls: &'a [ToolCall],
    tool_call_id: Option<&'a str>,
    reasoning: Option<&'a str>,
    keep_reasoning: bool,
    cached: bool,
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
            cached: false,
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
            cached: false,
        }
    }
}

impl Serialize for View<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("role", &self.role)?;
        self.content_entry(&mut map)?;
        if !self.tool_calls.is_empty() {
            map.serialize_entry("tool_calls", &Calls(self.tool_calls))?;
        }
        if let Some(id) = self.tool_call_id {
            map.serialize_entry("tool_call_id", id)?;
        }
        self.reasoning_entry(&mut map)?;
        map.end()
    }
}

impl View<'_, '_> {
    fn content_entry<M: SerializeMap>(&self, map: &mut M) -> std::result::Result<(), M::Error> {
        let cached = self.cached && (!self.content.is_empty() || !self.images.is_empty());
        if self.images.is_empty() && !cached {
            map.serialize_entry("content", self.content)
        } else {
            map.serialize_entry("content", &Parts(self.content, self.images, cached))
        }
    }

    fn reasoning_entry<M: SerializeMap>(&self, map: &mut M) -> std::result::Result<(), M::Error> {
        match (self.role, self.keep_reasoning, self.reasoning) {
            (Role::Assistant, true, reasoning) => {
                map.serialize_entry("reasoning_content", reasoning.unwrap_or_default())
            }
            (Role::Assistant, false, _) | (_, _, None) => Ok(()),
            (_, _, Some(reasoning)) => map.serialize_entry("reasoning_content", reasoning),
        }
    }
}

struct Parts<'a, 'b>(&'a str, &'b [&'a Image], bool);

impl Serialize for Parts<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut parts = Vec::with_capacity(self.1.len() + 1);
        if !self.0.is_empty() {
            parts.push(json!({ "type": "text", "text": self.0 }));
        }
        for image in self.1 {
            parts.push(json!({
                "type": "image_url",
                "image_url": { "url": image.url() }
            }));
        }
        if self.2
            && let Some(last) = parts.last_mut()
        {
            last["cache_control"] = json!({"type": "ephemeral"});
        }
        let mut seq = serializer.serialize_seq(Some(parts.len()))?;
        for part in &parts {
            seq.serialize_element(part)?;
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
