use super::Thinking::{Budget, Level};
use super::{Row, row, signed};
use crate::provider::Off::{ThinkingDisabled, Unsupported};
use crate::provider::levels::{LOW_HIGH, MINIMAL_UP, STANDARD};

pub(super) const ROWS: &[Row] = &[
    row(&["gemini-2.5", "pro"], Budget, STANDARD, Unsupported),
    row(&["gemini-2.5"], Budget, STANDARD, ThinkingDisabled),
    signed(&["gemini-3-pro"], Level, LOW_HIGH, Unsupported),
    signed(&["gemini-3", "pro"], Level, STANDARD, Unsupported),
    signed(&["gemini-3", "3.7"], Level, STANDARD, Unsupported),
    signed(&["gemini-3", "3.8"], Level, STANDARD, Unsupported),
    signed(&["gemini-3"], Level, MINIMAL_UP, Unsupported),
];
