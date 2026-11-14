use std::borrow::Cow;

use super::*;

#[derive(Clone, Debug)]
pub struct CompletionRequest<'a> {
    pub(crate) session: String,
    pub(crate) model: String,
    pub(crate) system_prompt: Option<Cow<'a, str>>,
    pub(crate) messages: &'a [Message],
    pub(crate) tools: &'a [ToolDefinition],
    pub(crate) max_output_tokens: Option<u32>,
    pub(crate) emit_deltas: bool,
    pub(crate) effort: Option<Effort>,
    #[cfg(test)]
    pub(crate) reasoning: Reasoning,
    pub(crate) traits: super::wire::traits::Traits,
    pub(crate) tools_json: Option<&'a ToolsJson>,
    pub(crate) prefix_hash: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountStyle {
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
pub fn compact(count: u64) -> String {
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
pub fn dollars(amount: f64) -> String {
    if amount < 0.01 {
        format!("${amount:.4}")
    } else {
        format!("${amount:.2}")
    }
}

impl<'a> CompletionRequest<'a> {
    #[must_use]
    pub(crate) fn new(
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
            system_prompt: None,
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
    pub(crate) fn with_system_prompt(mut self, system_prompt: impl Into<Cow<'a, str>>) -> Self {
        self.system_prompt = Some(system_prompt.into());
        self
    }

    #[cfg(test)]
    #[must_use]
    pub(super) fn with_model(mut self, model: String) -> Self {
        self.traits = super::wire::traits::traits(&model);
        self.model = model;
        self
    }

    #[must_use]
    pub(crate) fn with_tools_json(mut self, tools_json: &'a ToolsJson) -> Self {
        self.tools_json = Some(tools_json);
        self
    }

    #[must_use]
    pub(crate) fn with_prefix_hash(mut self, prefix_hash: String) -> Self {
        self.prefix_hash = Some(prefix_hash);
        self
    }

    pub(crate) fn shape_hash(&self) -> String {
        let effort = self.effort.map_or("", |effort| effort.as_str());
        summonr_core::text::fnv64(format!("{}\n{effort}", self.prefix_hash()).as_bytes())
    }

    pub(crate) fn prefix_hash(&self) -> String {
        if let Some(prefix_hash) = &self.prefix_hash {
            return prefix_hash.clone();
        }
        let system = self.system_prompt.as_deref().or_else(|| {
            self.messages
                .first()
                .map(|message| message.content.as_str())
        });
        tally(&self.model, system, self.tools)
            .map_or_else(|_| summonr_core::text::fnv64(&[]), |tally| tally.hash.hex())
    }
}

#[derive(Default)]
struct Tally {
    hash: summonr_core::text::Fnv64,
    len: usize,
}

impl std::io::Write for Tally {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        std::hash::Hasher::write(&mut self.hash, bytes);
        self.len += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The byte length of `value` as compact JSON, or 0 when it does not serialize.
pub(crate) fn encoded_len(value: &impl serde::Serialize) -> usize {
    let mut tally = Tally::default();
    serde_json::to_writer(&mut tally, value).map_or(0, |()| tally.len)
}

fn tally(model: &str, system: Option<&str>, tools: &[ToolDefinition]) -> std::io::Result<Tally> {
    use std::io::Write;
    let mut tally = Tally::default();
    tally.write_all(b"[")?;
    serde_json::to_writer(&mut tally, model)?;
    tally.write_all(b",")?;
    serde_json::to_writer(&mut tally, &system)?;
    tally.write_all(b",")?;
    serde_json::to_writer(&mut tally, tools)?;
    tally.write_all(b"]")?;
    Ok(tally)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ToolsJson {
    len: usize,
    sorted: std::sync::OnceLock<Vec<u8>>,
}

impl ToolsJson {
    #[must_use]
    pub(crate) fn encode(tools: &[ToolDefinition]) -> Self {
        Self {
            len: encoded_len(&tools),
            sorted: std::sync::OnceLock::new(),
        }
    }

    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn sorted(&self, tools: &[ToolDefinition]) -> &[u8] {
        self.sorted.get_or_init(|| {
            Self::value(tools)
                .and_then(|value| serde_json::to_vec(&value))
                .unwrap_or_default()
        })
    }

    /// The key-sorted JSON value of `tools`, the form every wire sends.
    pub(crate) fn value(tools: &[ToolDefinition]) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(tools)
    }
}
