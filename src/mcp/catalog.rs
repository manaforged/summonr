use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Listing {
    #[serde(default)]
    pub(super) tools: Vec<Value>,
    #[serde(default)]
    pub(super) prompts: Vec<Value>,
    #[serde(default)]
    pub(super) resources: Vec<Value>,
    #[serde(default)]
    pub(super) templates: Vec<Value>,
}

impl Listing {
    fn is_empty(&self) -> bool {
        self.tools.is_empty()
            && self.prompts.is_empty()
            && self.resources.is_empty()
            && self.templates.is_empty()
    }
}

fn path(key: &str) -> Option<PathBuf> {
    Some(
        crate::prompt::summonr_home()
            .ok()?
            .join("mcp")
            .join(format!("{key}.json")),
    )
}

pub(super) fn load(key: &str) -> Option<Listing> {
    let text = crate::durable::store::read_or_empty(&path(key)?).ok()?;
    serde_json::from_str::<Listing>(&text)
        .ok()
        .filter(|listing| !listing.is_empty())
}

pub(super) fn save(key: &str, listing: &Listing) {
    if load(key).is_some_and(|cached| cached == *listing) {
        return;
    }
    if let (Some(path), Ok(text)) = (path(key), serde_json::to_string(listing)) {
        drop(crate::durable::store::write(&path, text));
    }
}
