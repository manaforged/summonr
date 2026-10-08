use super::Thinking::{Adaptive, Budgeted, EffortBudget};
use super::{NONE, Row, claude, row};
use crate::provider::Off::{BetweenTools, Omit, ThinkingDisabled, Unsupported};
use crate::provider::levels::{FULL, HIGH_MAX, STANDARD};

pub(super) const ROWS: &[Row] = &[
    claude(row(&["claude-opus-5-5"], Adaptive, FULL, Unsupported)),
    claude(row(&["claude-opus-5"], Adaptive, FULL, ThinkingDisabled)),
    claude(row(&["claude-sonnet-5-5"], Adaptive, FULL, BetweenTools)),
    claude(row(&["claude-sonnet-5"], Adaptive, FULL, ThinkingDisabled)),
    claude(row(&["claude-fable-5"], Adaptive, FULL, Unsupported)),
    claude(row(&["claude-mythos-5"], Adaptive, FULL, Unsupported)),
    claude(row(
        &["claude-mythos-preview"],
        Adaptive,
        HIGH_MAX,
        Unsupported,
    )),
    claude(row(&["claude-opus-4-8"], Adaptive, FULL, Omit)),
    claude(row(&["claude-opus-4-7"], Adaptive, FULL, Omit)),
    claude(row(&["claude-opus-4-6"], Adaptive, HIGH_MAX, Omit)),
    claude(row(&["claude-sonnet-4-6"], Adaptive, HIGH_MAX, Omit)),
    claude(row(&["claude-opus-4-5"], EffortBudget, STANDARD, Omit)),
    claude(row(&["claude-3-7"], Budgeted, STANDARD, Omit)),
    claude(row(&["claude-sonnet-4-5"], Budgeted, STANDARD, Omit)),
    claude(row(&["claude-haiku-4-5"], Budgeted, STANDARD, Omit)),
    claude(row(&["claude-opus-4-1"], Budgeted, STANDARD, Omit)),
    claude(row(&["claude-opus-4-0"], Budgeted, STANDARD, Omit)),
    claude(row(&["claude-sonnet-4-0"], Budgeted, STANDARD, Omit)),
    claude(row(&["claude-opus-4-2025"], Budgeted, STANDARD, Omit)),
    claude(row(&["claude-sonnet-4-2025"], Budgeted, STANDARD, Omit)),
    claude(Row {
        parts: &["claude"],
        traits: NONE,
    }),
];
