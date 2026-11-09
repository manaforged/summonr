use std::path::Path;

use crate::durable::store;
use crate::provider::Effort;
use summonr_core::error::Result;

const FILE: &str = "recent.json";
const PINNED: &str = "favorites.json";
const CAP: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ModelId {
    pub endpoint: String,
    pub model: String,
}

impl ModelId {
    pub(crate) fn new(endpoint: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
        }
    }

    fn key(&self) -> String {
        format!("{}\t{}", self.endpoint, self.model)
    }
}

struct Models;

impl store::Codec for Models {
    type Value = Vec<(ModelId, Option<Effort>)>;

    fn decode(path: &Path, text: &str) -> Result<Self::Value> {
        let rows: Vec<String> = serde_json::from_str(text).map_err(|error| {
            summonr_core::error::Error::Config(format!("{}: {error}", crate::config::tilde(path)))
        })?;
        Ok(rows
            .iter()
            .filter_map(|row| {
                let mut fields = row.splitn(3, '\t');
                let selection = ModelId::new(fields.next()?, fields.next()?);
                Some((selection, fields.next().and_then(Effort::parse)))
            })
            .collect())
    }
}

impl store::Encode for Models {
    fn encode(selections: &Self::Value) -> Result<String> {
        let rows: Vec<String> = selections
            .iter()
            .map(|(selection, effort)| match effort {
                Some(effort) => format!("{}\t{}", selection.key(), effort.as_str()),
                None => selection.key(),
            })
            .collect();
        Ok(serde_json::to_string(&rows)?)
    }
}

fn doc(home: &Path, file: &str) -> store::Doc<Models> {
    store::Doc::new(
        home.join(file),
        store::Durability::Renamed,
        store::Invalid::Warn,
        store::Wait::Forever,
    )
}

fn rows(home: &Path, file: &str) -> Vec<(ModelId, Option<Effort>)> {
    doc(home, file).load().unwrap_or_default()
}

fn read(home: &Path, file: &str) -> Vec<ModelId> {
    rows(home, file)
        .into_iter()
        .map(|(selection, _)| selection)
        .collect()
}

fn update(
    home: &Path,
    file: &str,
    change: impl FnOnce(&mut Vec<(ModelId, Option<Effort>)>),
) -> Result<()> {
    doc(home, file)
        .update(|selections| {
            change(selections);
            Ok(true)
        })
        .map(drop)
}

pub(crate) fn load(home: &Path) -> Vec<ModelId> {
    read(home, FILE)
}

pub(crate) fn pinned(home: &Path) -> Vec<ModelId> {
    read(home, PINNED)
}

pub(crate) fn record(home: &Path, selection: &ModelId, effort: Option<Effort>) -> Result<()> {
    update(home, FILE, |selections| {
        selections.retain(|(existing, _)| existing != selection);
        selections.insert(0, (selection.clone(), effort));
        selections.truncate(CAP);
    })
}

pub(crate) fn pin(home: &Path, selection: &ModelId, on: bool) -> Result<()> {
    update(home, PINNED, |selections| {
        selections.retain(|(existing, _)| existing != selection);
        if on {
            selections.push((selection.clone(), None));
        }
    })
}

pub(crate) fn remembered_effort(
    home: Option<&Path>,
    endpoint: &str,
    model: &str,
) -> Option<Effort> {
    let selection = ModelId::new(endpoint, model);
    rows(home?, FILE)
        .into_iter()
        .find(|(row, _)| *row == selection)
        .and_then(|(_, effort)| effort)
}

#[must_use]
pub fn default_effort(endpoint: &str, model: &str) -> Option<Effort> {
    let home = summonr_core::dirs::summonr_home().ok();
    remembered_effort(home.as_deref(), endpoint, model).or_else(|| {
        crate::catalog::known(endpoint, model)
            .filter(|known| known.reasoning)
            .map(|_| Effort::High)
    })
}

#[cfg(test)]
#[path = "recent_tests.rs"]
mod tests;
