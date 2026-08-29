use serde::Deserialize;
use serde_json::Value;

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

#[derive(Default)]
pub(super) struct Scan {
    inside: bool,
    hold: String,
}

impl Scan {
    pub(super) fn push(
        &mut self,
        part: &str,
        mut on_text: impl FnMut(&str),
        mut on_think: impl FnMut(&str),
    ) {
        self.hold.push_str(part);
        loop {
            let tag = if self.inside { CLOSE } else { OPEN };
            if let Some(at) = self.hold.find(tag) {
                let before = self.hold[..at].to_string();
                if !before.is_empty() {
                    emit(self.inside, &before, &mut on_text, &mut on_think);
                }
                self.hold.drain(..at + tag.len());
                self.inside = !self.inside;
                continue;
            }
            let keep = prefix_len(&self.hold, tag);
            let emit_at = self.hold.len() - keep;
            if emit_at > 0 {
                let chunk = self.hold[..emit_at].to_string();
                emit(self.inside, &chunk, &mut on_text, &mut on_think);
                self.hold.drain(..emit_at);
            }
            break;
        }
    }

    pub(super) fn finish(self, mut on_text: impl FnMut(&str), mut on_think: impl FnMut(&str)) {
        if !self.hold.is_empty() {
            emit(self.inside, &self.hold, &mut on_text, &mut on_think);
        }
    }
}

fn emit(inside: bool, part: &str, on_text: &mut impl FnMut(&str), on_think: &mut impl FnMut(&str)) {
    if inside {
        on_think(part);
    } else {
        on_text(part);
    }
}

fn prefix_len(buf: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|&n| buf.ends_with(&tag[..n]))
        .unwrap_or(0)
}

pub(super) fn split(content: &str) -> (String, String) {
    let mut text = String::new();
    let mut thought = String::new();
    let mut scan = Scan::default();
    scan.push(
        content,
        |part| text.push_str(part),
        |part| thought.push_str(part),
    );
    scan.finish(|part| text.push_str(part), |part| thought.push_str(part));
    (text, thought)
}

pub(super) fn hoist(message: &mut super::AssistantMessage) {
    let Some(content) = message.content.take() else {
        return;
    };
    let (text, thought) = split(&content);
    message.content = empty_to_none(text);
    if thought.is_empty() {
        return;
    }
    if message
        .reasoning_content
        .as_deref()
        .is_none_or(str::is_empty)
    {
        message.reasoning_content = Some(thought);
    }
}

pub(super) fn reasoning_field<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(from_json(value.unwrap_or(Value::Null)))
}

fn from_json(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => empty_to_none(text),
        Value::Object(map) => map
            .get("content")
            .or_else(|| map.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .and_then(empty_to_none),
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                if let Some(text) = part.as_str() {
                    out.push_str(text);
                } else if let Some(text) = part.get("text").and_then(Value::as_str) {
                    out.push_str(text);
                } else if let Some(text) = part.get("content").and_then(Value::as_str) {
                    out.push_str(text);
                }
            }
            empty_to_none(out)
        }
        _ => None,
    }
}

fn empty_to_none(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
#[path = "think_tests.rs"]
mod tests;
