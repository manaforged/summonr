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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl<'de> Deserialize<'de> for Effort {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("effort {}", Self::rejects(&text))))
    }
}

impl Effort {
    pub(crate) const ALL: [Self; 7] = [
        Self::None,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
    ];

    #[must_use]
    pub fn rejects(text: &str) -> String {
        let names: Vec<&str> = Self::ALL.iter().map(|effort| effort.as_str()).collect();
        let (last, rest) = names.split_last().unwrap_or((&"", &[]));
        format!("takes {}, or {last}, not `{text}`", rest.join(", "))
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Minimal,
            Self::Minimal => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::XHigh,
            Self::XHigh => Self::Max,
            Self::Max => Self::None,
        }
    }

    #[must_use]
    pub(crate) fn clamp(self, levels: &[Self]) -> Self {
        levels
            .iter()
            .copied()
            .min_by_key(|level| ((*level as u8).abs_diff(self as u8), *level))
            .unwrap_or(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cached: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

impl Price {
    #[must_use]
    pub(crate) fn cost(&self, prompt: u64, completion: u64, cached: u64, written: u64) -> f64 {
        let cached = cached.min(prompt);
        let written = written.min(prompt - cached);
        let fresh = prompt - cached - written;
        (fresh as f64 * self.input
            + cached as f64 * self.cached
            + written as f64 * self.cache_write.unwrap_or(self.input)
            + completion as f64 * self.output)
            / 1_000_000.0
    }
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

    fn tools_bytes(&self) -> std::borrow::Cow<'a, [u8]> {
        self.tools_json.map_or_else(
            || std::borrow::Cow::Owned(serde_json::to_vec(self.tools).unwrap_or_default()),
            |encoded| std::borrow::Cow::Borrowed(encoded.declared.as_slice()),
        )
    }

    pub(crate) fn prefix_hash(&self) -> String {
        let system = self.messages.first().map(|message| &message.content);
        let tools = self.tools_bytes();
        let mut bytes = Vec::with_capacity(tools.len() + self.model.len() + 64);
        bytes.push(b'[');
        let written = serde_json::to_writer(&mut bytes, &self.model).is_ok() && {
            bytes.push(b',');
            serde_json::to_writer(&mut bytes, &system).is_ok()
        };
        if !written {
            return crate::text::fnv64(&[]);
        }
        bytes.push(b',');
        bytes.extend_from_slice(&tools);
        bytes.push(b']');
        crate::text::fnv64(&bytes)
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

#[derive(Clone, Debug, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: ToolKind,
    pub function: FunctionDefinition,
}

#[derive(Clone, Debug, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[allow(
    async_fn_in_trait,
    reason = "Provider is an application-owned trait with no public Send contract"
)]
#[derive(Default)]
pub struct Partial {
    pub text: String,
    pub reasoning: String,
}
