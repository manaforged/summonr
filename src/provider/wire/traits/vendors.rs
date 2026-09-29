use super::{Row, scaled};
use crate::provider::Off::{EffortNone, Omit, ThinkingDisabled, Unsupported};
use crate::provider::levels::{ALL_POSITIVE, EXTENDED, LOW_HIGH_MAX, NO_EFFORT, STANDARD};

pub(super) const ROWS: &[Row] = &[
    scaled(&["glm-5.3"], LOW_HIGH_MAX, Unsupported),
    scaled(&["glm-5.2"], ALL_POSITIVE, EffortNone),
    scaled(&["glm-"], NO_EFFORT, ThinkingDisabled),
    scaled(&["kimi-k3"], LOW_HIGH_MAX, Unsupported),
    scaled(&["k2.6"], NO_EFFORT, ThinkingDisabled),
    scaled(&["kimi-k2"], NO_EFFORT, Omit),
    scaled(&["grok-4.7"], EXTENDED, Unsupported),
    scaled(&["grok-4.6"], EXTENDED, Unsupported),
    scaled(&["grok-4.5"], STANDARD, Unsupported),
    scaled(&["gpt-oss"], STANDARD, Unsupported),
];
