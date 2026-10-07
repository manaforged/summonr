use super::{Row, replayed, scaled, zai};
use crate::provider::Off::{EffortNone, Omit, ThinkingDisabled, Unsupported};
use crate::provider::levels::{ALL_POSITIVE, EXTENDED, LOW_HIGH_MAX, NO_EFFORT, STANDARD};

pub(super) const ROWS: &[Row] = &[
    zai(scaled(&["glm-5.3"], LOW_HIGH_MAX, Unsupported)),
    zai(scaled(&["glm-5.2"], ALL_POSITIVE, EffortNone)),
    zai(scaled(&["glm-"], NO_EFFORT, ThinkingDisabled)),
    replayed(scaled(&["deepseek"], LOW_HIGH_MAX, ThinkingDisabled)),
    scaled(&["kimi-k3"], LOW_HIGH_MAX, Unsupported),
    scaled(&["k2.6"], NO_EFFORT, ThinkingDisabled),
    scaled(&["kimi-k2"], NO_EFFORT, Omit),
    replayed(scaled(&["grok-4.7"], EXTENDED, Unsupported)),
    replayed(scaled(&["grok-4-7"], EXTENDED, Unsupported)),
    replayed(scaled(&["grok-4.6"], EXTENDED, Unsupported)),
    replayed(scaled(&["grok-4-6"], EXTENDED, Unsupported)),
    replayed(scaled(&["grok-4.5"], STANDARD, Unsupported)),
    replayed(scaled(&["grok-4-5"], STANDARD, Unsupported)),
    scaled(&["gpt-oss"], STANDARD, Unsupported),
];
