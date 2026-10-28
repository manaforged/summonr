use super::*;

#[derive(Clone, Debug)]
pub struct CompletionRequest<'a> {
    pub session: String,
    pub model: String,
    pub messages: &'a [Message],
    pub tools: &'a [ToolDefinition],
    pub max_output_tokens: Option<u32>,
    pub emit_deltas: bool,
    pub effort: Option<Effort>,
    #[cfg(test)]
    pub(crate) reasoning: Reasoning,
    pub(crate) traits: super::wire::traits::Traits,
    pub(crate) tools_json: Option<&'a ToolsJson>,
    pub(crate) prefix_hash: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CountStyle {
    Usage,
    Window,
}

const WINDOW_MILLIONS_FROM: u64 = 950_000;
const WINDOW_ROUND_SLACK: f64 = 0.1;

impl CountStyle {
    #[must_use]
    pub(crate) fn format(self, count: u64) -> String {
        match self {
            Self::Usage => usage_count(count),
            Self::Window => window_count(count),
        }
    }
}

#[must_use]
pub(crate) fn compact(count: u64) -> String {
    CountStyle::Usage.format(count)
}

fn usage_count(count: u64) -> String {
    let value = count as f64;
    if value >= 1e9 {
        format!("{:.2}b", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.1}m", value / 1e6)
    } else if value >= 1e3 {
        format!("{:.1}k", value / 1e3)
    } else {
        count.to_string()
    }
}

fn window_count(count: u64) -> String {
    if count >= WINDOW_MILLIONS_FROM {
        let millions = count as f64 / 1e6;
        if (millions - millions.round()).abs() < WINDOW_ROUND_SLACK {
            format!("{}M", millions.round())
        } else {
            format!("{millions:.1}M")
        }
    } else if count >= 1000 {
        format!("{}k", count / 1000)
    } else {
        count.to_string()
    }
}

#[must_use]
pub(crate) fn dollars(amount: f64) -> String {
    if amount < 0.01 {
        format!("${amount:.4}")
    } else {
        format!("${amount:.2}")
    }
}

impl<'a> CompletionRequest<'a> {
    #[must_use]
    pub fn new(
        session: String,
        model: String,
        messages: &'a [Message],
        tools: &'a [ToolDefinition],
        max_output_tokens: Option<u32>,
        emit_deltas: bool,
        effort: Option<Effort>,
    ) -> Self {
        Self {
            session,
            messages,
            tools,
            max_output_tokens,
            emit_deltas,
            effort,
            #[cfg(test)]
            reasoning: Reasoning::default(),
            traits: super::wire::traits::traits(&model),
            tools_json: None,
            prefix_hash: None,
            model,
        }
    }

    #[must_use]
    pub fn with_model(mut self, model: String) -> Self {
        self.traits = super::wire::traits::traits(&model);
        self.model = model;
        self
    }

    #[must_use]
    pub fn with_tools_json(mut self, tools_json: &'a ToolsJson) -> Self {
        self.tools_json = Some(tools_json);
        self
    }

    #[must_use]
    pub fn with_prefix_hash(mut self, prefix_hash: String) -> Self {
        self.prefix_hash = Some(prefix_hash);
        self
    }

    fn tools_bytes(&self) -> std::borrow::Cow<'a, [u8]> {
        self.tools_json.map_or_else(
            || std::borrow::Cow::Owned(serde_json::to_vec(self.tools).unwrap_or_default()),
            |encoded| std::borrow::Cow::Borrowed(encoded.declared.as_slice()),
        )
    }

    pub(crate) fn shape_hash(&self) -> String {
        let effort = self.effort.map_or("", |effort| effort.as_str());
        summonr_core::text::fnv64(format!("{}\n{effort}", self.prefix_hash()).as_bytes())
    }

    pub(crate) fn prefix_hash(&self) -> String {
        if let Some(prefix_hash) = &self.prefix_hash {
            return prefix_hash.clone();
        }
        let system = self.messages.first().map(|message| &message.content);
        let tools = self.tools_bytes();
        let mut bytes = Vec::with_capacity(tools.len() + self.model.len() + 64);
        bytes.push(b'[');
        let written = serde_json::to_writer(&mut bytes, &self.model).is_ok() && {
            bytes.push(b',');
            serde_json::to_writer(&mut bytes, &system).is_ok()
        };
        if !written {
            return summonr_core::text::fnv64(&[]);
        }
        bytes.push(b',');
        bytes.extend_from_slice(&tools);
        bytes.push(b']');
        summonr_core::text::fnv64(&bytes)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ToolsJson {
    declared: Vec<u8>,
    pub(crate) sorted: Vec<u8>,
}

impl ToolsJson {
    #[must_use]
    pub fn encode(tools: &[ToolDefinition]) -> Self {
        let declared = serde_json::to_vec(tools).unwrap_or_default();
        let sorted = serde_json::to_value(tools)
            .and_then(|value| serde_json::to_vec(&value))
            .unwrap_or_default();
        Self { declared, sorted }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.declared.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.declared.is_empty()
    }
}
