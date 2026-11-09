use std::collections::BTreeMap;
use std::path::Path;

use crate::config;
use summonr_core::error::Result;

pub(crate) use summonr_types::modules::ALL;
pub use summonr_types::modules::{Module, ModuleId, Switches, named};

pub fn load(workspace: &Path) -> Result<Switches> {
    let user = config::file()?.modules.clone();
    let path = workspace.join(".summonr/config.toml");
    let project = if path.is_file() {
        config::parse(&path)?.modules
    } else {
        BTreeMap::new()
    };
    Ok(Switches::from_maps(&user, &project))
}

pub fn set(id: ModuleId, on: bool) -> Result<()> {
    config::save_module(&config::path()?, id, on)
}
