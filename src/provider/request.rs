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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Low,
    Medium,
    High,
}

impl<'de> Deserialize<'de> for Effort {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "effort takes none, low, medium, or high, not `{text}`"
            ))
        })
    }
}

impl Effort {
    pub const ALL: [Self; 4] = [Self::None, Self::Low, Self::Medium, Self::High];

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cached: f64,
}

impl Price {
    #[must_use]
    pub(crate) fn cost(&self, prompt: u64, completion: u64, cached: u64) -> f64 {
        let cached = cached.min(prompt);
        let fresh = prompt - cached;
        (fresh as f64 * self.input + cached as f64 * self.cached + completion as f64 * self.output)
            / 1_000_000.0
    }
}

#[must_use]
pub(crate) fn compact(count: u64) -> String {
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

#[must_use]
pub fn dollars(amount: f64) -> String {
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
            model,
            messages,
            tools,
            max_output_tokens,
            emit_deltas,
            effort,
        }
    }

    pub(crate) fn prefix_hash(&self) -> String {
        let system = self.messages.first().map(|message| &message.content);
        let bytes = serde_json::to_vec(&(&self.model, system, &self.tools)).unwrap_or_default();
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}")
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
