use super::Thinking::{Adaptive, Budgeted, EffortBudget};
use super::{Row, row};
use crate::provider::Off::{BetweenTools, Omit, ThinkingDisabled, Unsupported};
use crate::provider::levels::{FULL, HIGH_MAX, STANDARD};

pub(super) const ROWS: &[Row] = &[
    row(&["claude-opus-5-5"], Adaptive, FULL, Unsupported),
    row(&["claude-opus-5"], Adaptive, FULL, ThinkingDisabled),
    row(&["claude-sonnet-5-5"], Adaptive, FULL, BetweenTools),
    row(&["claude-sonnet-5"], Adaptive, FULL, ThinkingDisabled),
    row(&["claude-fable-5"], Adaptive, FULL, Unsupported),
    row(&["claude-mythos-5"], Adaptive, FULL, Unsupported),
    row(&["claude-mythos-preview"], Adaptive, HIGH_MAX, Unsupported),
    row(&["claude-opus-4-8"], Adaptive, FULL, Omit),
    row(&["claude-opus-4-7"], Adaptive, FULL, Omit),
    row(&["claude-opus-4-6"], Adaptive, HIGH_MAX, Omit),
    row(&["claude-sonnet-4-6"], Adaptive, HIGH_MAX, Omit),
    row(&["claude-opus-4-5"], EffortBudget, STANDARD, Omit),
    row(&["claude-3-7"], Budgeted, STANDARD, Omit),
    row(&["claude-sonnet-4-5"], Budgeted, STANDARD, Omit),
    row(&["claude-haiku-4-5"], Budgeted, STANDARD, Omit),
    row(&["claude-opus-4-1"], Budgeted, STANDARD, Omit),
    row(&["claude-opus-4-0"], Budgeted, STANDARD, Omit),
    row(&["claude-sonnet-4-0"], Budgeted, STANDARD, Omit),
    row(&["claude-opus-4-2025"], Budgeted, STANDARD, Omit),
    row(&["claude-sonnet-4-2025"], Budgeted, STANDARD, Omit),
];
