use super::{Row, scaled};
use crate::provider::Off::{EffortNone, Unsupported};
use crate::provider::levels::{
    EXTENDED, FULL, HIGH_ONLY, MEDIUM_UP, MINIMAL_UP, NO_EFFORT, STANDARD,
};

pub(super) const ROWS: &[Row] = &[
    scaled(&["gpt-6-astra"], FULL, Unsupported),
    scaled(&["gpt-6"], FULL, EffortNone),
    scaled(&["gpt-5.6"], FULL, EffortNone),
    scaled(&["gpt-5-pro"], HIGH_ONLY, Unsupported),
    scaled(&["gpt-5", "-pro"], MEDIUM_UP, Unsupported),
    scaled(&["gpt-5.3-codex"], EXTENDED, Unsupported),
    scaled(&["gpt-5.2-codex"], EXTENDED, Unsupported),
    scaled(&["codex"], STANDARD, Unsupported),
    scaled(&["gpt-5.5"], EXTENDED, EffortNone),
    scaled(&["gpt-5.4"], EXTENDED, EffortNone),
    scaled(&["gpt-5.2"], EXTENDED, EffortNone),
    scaled(&["gpt-5.1"], STANDARD, EffortNone),
    scaled(&["gpt-5-chat"], NO_EFFORT, Unsupported),
    scaled(&["gpt-5"], MINIMAL_UP, Unsupported),
    scaled(&["gpt-4"], NO_EFFORT, Unsupported),
    scaled(&["gpt-3"], NO_EFFORT, Unsupported),
];
