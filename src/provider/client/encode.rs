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

    pub(super) fn take(&mut self, key: &str) -> Option<Value> {
        self.head.remove(key)
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
