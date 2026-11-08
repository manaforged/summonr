use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::durable::store;
use summonr_core::dirs::summonr_home;
use summonr_core::error::Result;

const FILE: &str = "desktop.json";

/// What a restart restores: panes, terminals and the marks on agents out of view.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub panes: Vec<Slot>,
    pub shape: Option<super::Shape>,
    pub focus: usize,
    pub terminals: Vec<PathBuf>,
    pub marks: BTreeMap<String, Mark>,
    pub pinned: Vec<String>,
    pub archived: Vec<String>,
    pub spaces: Vec<Space>,
}

/// A workspace a person made: its name, the folders it holds and the agents put in it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Space {
    pub id: String,
    pub name: String,
    pub folders: Vec<PathBuf>,
    pub sessions: Vec<String>,
}

/// One pane: its agent, and the index of the terminal it shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Slot {
    pub session: String,
    pub terminal: Option<usize>,
}

/// The status an agent keeps across a restart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Review,
    Failed,
}

fn doc() -> Result<store::Doc<store::Json<Layout>>> {
    Ok(store::Doc::new(
        summonr_home()?.join(FILE),
        store::Durability::Synced,
        store::Invalid::Warn,
        store::Wait::Never,
    ))
}

impl Space {
    /// An empty workspace named `name`, with a new id.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::now_v7().simple().to_string(),
            name: name.into(),
            ..Self::default()
        }
    }
}

impl Layout {
    /// The saved layout, or an empty one when none can be read.
    pub fn load() -> Self {
        doc().and_then(|doc| doc.load()).unwrap_or_default()
    }

    /// Saves this layout.
    pub fn save(&self) -> Result<()> {
        doc()?.replace(self)
    }
}
