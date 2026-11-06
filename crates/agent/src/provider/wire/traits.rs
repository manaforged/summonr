use std::sync::LazyLock;

use serde::{Deserialize, Deserializer};

use crate::provider::{Effort, Off, Scale};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Thinking {
    Adaptive,
    Budgeted,
    EffortBudget,
    Budget,
    Level,
    #[default]
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Family {
    #[default]
    Other,
    Anthropic,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Traits {
    pub(crate) family: Family,
    pub(crate) thinking: Thinking,
    pub(crate) skip_signature: bool,
    #[serde(deserialize_with = "scale")]
    pub(crate) scale: Option<Scale>,
    pub(crate) replay: bool,
    pub(crate) preserved: bool,
    pub(crate) tool_stream: bool,
    pub(crate) tool_images: bool,
    pub(crate) bound: bool,
    pub(crate) output: u32,
    pub(crate) patch: bool,
    #[serde(deserialize_with = "budgets")]
    pub(crate) thinking_budgets: &'static [(Effort, i32)],
}

fn budgets<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<&'static [(Effort, i32)], D::Error> {
    let rows = std::collections::BTreeMap::<Effort, i32>::deserialize(deserializer)?;
    Ok(rows.into_iter().collect::<Vec<_>>().leak())
}

#[derive(Deserialize)]
struct ScaleRow {
    levels: Vec<Effort>,
    off: Off,
    #[serde(default)]
    up: bool,
}

fn scale<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Scale>, D::Error> {
    let row = ScaleRow::deserialize(deserializer)?;
    let scale = Scale::new(row.levels.leak(), row.off);
    Ok(Some(if row.up { scale.upward() } else { scale }))
}

#[derive(Deserialize)]
struct Row {
    parts: Vec<String>,
    #[serde(flatten)]
    traits: Traits,
}

#[derive(Deserialize)]
struct Catalog {
    rows: Vec<Row>,
}

static ROWS: LazyLock<Vec<Row>> = LazyLock::new(|| {
    summonr_core::table::embedded::<Catalog>("model", include_str!("models.toml")).rows
});

pub(crate) fn traits(model: &str) -> Traits {
    let model = model.to_ascii_lowercase();
    ROWS.iter()
        // complexity: bounded by the 68 catalog rows in models.toml and their few name parts
        .find(|row| row.parts.iter().all(|part| model.contains(part.as_str())))
        .map_or_else(Traits::default, |row| row.traits)
}
