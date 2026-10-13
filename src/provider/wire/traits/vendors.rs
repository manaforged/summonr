use super::{Row, replayed, scaled, zai};
use crate::provider::Off::{EffortNone, EnableThinking, Omit, ThinkingDisabled, Unsupported};
use crate::provider::levels::{ALL_POSITIVE, EXTENDED, LOW_HIGH_MAX, NO_EFFORT, STANDARD};

pub(super) const ROWS: &[Row] = &[
    zai(scaled(&["glm-5.3"], LOW_HIGH_MAX, Unsupported)),
    zai(scaled(&["glm-5.2"], ALL_POSITIVE, EffortNone)),
    zai(scaled(&["glm-"], NO_EFFORT, ThinkingDisabled)),
    replayed(upward(scaled(
        &["deepseek"],
        LOW_HIGH_MAX,
        ThinkingDisabled,
    ))),
    replayed(scaled(
        &["kimi-for-coding-highspeed"],
        NO_EFFORT,
        Unsupported,
    )),
    replayed(scaled(&["kimi-for-coding"], LOW_HIGH_MAX, ThinkingDisabled)),
    replayed(scaled(&["k3"], LOW_HIGH_MAX, Unsupported)),
    replayed(scaled(&["k2.7-code"], NO_EFFORT, Omit)),
    scaled(&["k2.6"], NO_EFFORT, ThinkingDisabled),
    scaled(&["kimi-k2"], NO_EFFORT, Omit),
    replayed(scaled(&["grok-4.7"], EXTENDED, Unsupported)),
    replayed(scaled(&["grok-4-7"], EXTENDED, Unsupported)),
    replayed(scaled(&["grok-4.6"], EXTENDED, Unsupported)),
    replayed(scaled(&["grok-4-6"], EXTENDED, Unsupported)),
    replayed(scaled(&["grok-4.5"], STANDARD, Unsupported)),
    replayed(scaled(&["grok-4-5"], STANDARD, Unsupported)),
    scaled(&["gpt-oss"], STANDARD, Unsupported),
    replayed(scaled(&["minimax-m"], NO_EFFORT, Omit)),
    scaled(&["qwen3.", "plus"], STANDARD, EnableThinking),
    scaled(&["qwen3.", "max"], STANDARD, EnableThinking),
];

const fn upward(row: Row) -> Row {
    Row {
        parts: row.parts,
        traits: super::Traits {
            scale: match row.traits.scale {
                Some(scale) => Some(scale.upward()),
                None => None,
            },
            ..row.traits
        },
    }
}
