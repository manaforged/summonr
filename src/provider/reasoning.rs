use serde::Deserialize;
use serde_json::{Value, json};

use super::Effort;
use super::wire::traits::Traits;

pub(crate) mod levels {
    use crate::provider::Effort;

    pub(crate) const STANDARD: &[Effort] = &[Effort::Low, Effort::Medium, Effort::High];
    pub(crate) const EXTENDED: &[Effort] =
        &[Effort::Low, Effort::Medium, Effort::High, Effort::XHigh];
    pub(crate) const HIGH_MAX: &[Effort] =
        &[Effort::Low, Effort::Medium, Effort::High, Effort::Max];
    pub(crate) const FULL: &[Effort] = &[
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];
    pub(crate) const MINIMAL_UP: &[Effort] =
        &[Effort::Minimal, Effort::Low, Effort::Medium, Effort::High];
    pub(crate) const ALL_POSITIVE: &[Effort] = &[
        Effort::Minimal,
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];
    pub(crate) const LOW_HIGH: &[Effort] = &[Effort::Low, Effort::High];
    pub(crate) const LOW_HIGH_MAX: &[Effort] = &[Effort::Low, Effort::High, Effort::Max];
    pub(crate) const MEDIUM_UP: &[Effort] = &[Effort::Medium, Effort::High, Effort::XHigh];
    pub(crate) const HIGH_ONLY: &[Effort] = &[Effort::High];
    pub(crate) const NO_EFFORT: &[Effort] = &[];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Off {
    Omit,
    #[serde(rename = "none")]
    EffortNone,
    #[serde(rename = "disabled")]
    ThinkingDisabled,
    BetweenTools,
    #[serde(rename = "chat_template_kwargs")]
    TemplateKwarg,
    EnableThinking,
    Unsupported,
}

impl Off {
    pub(crate) fn fragment(self) -> Option<(&'static str, Value)> {
        match self {
            Self::ThinkingDisabled => Some(("thinking", json!({"type": "disabled"}))),
            Self::BetweenTools => Some(("thinking", json!({"type": "between_tools"}))),
            Self::TemplateKwarg => {
                Some(("chat_template_kwargs", json!({"enable_thinking": false})))
            }
            Self::EnableThinking => Some(("enable_thinking", json!(false))),
            Self::Omit | Self::EffortNone | Self::Unsupported => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EffortField {
    ReasoningEffort,
    ReasoningObject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Scale {
    pub(crate) levels: &'static [Effort],
    pub(crate) off: Off,
    pub(crate) up: bool,
}

impl Scale {
    pub(crate) const fn new(levels: &'static [Effort], off: Off) -> Self {
        Self {
            levels,
            off,
            up: false,
        }
    }

    pub(crate) const fn upward(self) -> Self {
        Self { up: true, ..self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    pub(crate) field: EffortField,
    pub(crate) scale: Scale,
    pub(crate) by_model: bool,
}

impl Spec {
    pub(crate) const fn new(field: EffortField, levels: &'static [Effort], off: Off) -> Self {
        Self {
            field,
            scale: Scale::new(levels, off),
            by_model: true,
        }
    }
}

impl Default for Spec {
    fn default() -> Self {
        Self::new(EffortField::ReasoningEffort, levels::STANDARD, Off::Omit)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Reasoning {
    pub(crate) spec: Spec,
    pub(crate) levels: Option<Vec<Effort>>,
    pub(crate) off: Option<Off>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Default,
    Level(Effort),
    Off(Off),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Plan<'a> {
    pub(crate) field: EffortField,
    pub(crate) levels: &'a [Effort],
    pub(crate) off: Off,
    pub(crate) up: bool,
}

impl Reasoning {
    pub(crate) fn levels_for(&self, model: &str) -> Vec<Effort> {
        self.plan(super::wire::traits::traits(model))
            .levels
            .to_vec()
    }

    pub(crate) fn plan(&self, traits: Traits) -> Plan<'_> {
        let row = traits.scale.filter(|_| self.spec.by_model);
        Plan {
            field: self.spec.field,
            levels: self
                .levels
                .as_deref()
                .or(row.map(|scale| scale.levels))
                .unwrap_or(self.spec.scale.levels),
            off: self
                .off
                .or(row.map(|scale| scale.off))
                .unwrap_or(self.spec.scale.off),
            up: self.levels.is_none() && row.is_some_and(|scale| scale.up),
        }
    }
}

impl Plan<'_> {
    pub(crate) fn step(&self, requested: Option<Effort>) -> Step {
        match requested {
            None => Step::Default,
            Some(Effort::None) => self.disabled(),
            Some(_) if self.levels.is_empty() => Step::Default,
            Some(level) => Step::Level(level.rounded(self.levels, self.up)),
        }
    }

    fn disabled(&self) -> Step {
        match self.off {
            Off::Omit => Step::Default,
            Off::EffortNone => Step::Level(Effort::None),
            Off::ThinkingDisabled
            | Off::BetweenTools
            | Off::TemplateKwarg
            | Off::EnableThinking => Step::Off(self.off),
            Off::Unsupported => self
                .levels
                .iter()
                .min()
                .map_or(Step::Default, |lowest| Step::Level(*lowest)),
        }
    }

    pub(crate) fn budget(&self, level: Effort, most: u32) -> u32 {
        let below = self.levels.iter().filter(|other| **other < level).count();
        let steps = u32::try_from(self.levels.len() + 1).unwrap_or(u32::MAX);
        let rung = u32::try_from(below + 1).unwrap_or(u32::MAX);
        most / steps * rung
    }
}
