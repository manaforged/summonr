use std::path::Path;

use crate::config;
use summonr_core::error::Result;

pub use summonr_types::modules::ALL;
pub use summonr_types::modules::{Module, ModuleId, Switches, named};

pub fn load(workspace: &Path) -> Result<Switches> {
    let user = config::file()?.modules.clone();
    let project = config::workspace(workspace)?;
    Ok(Switches::from_maps(&user, &project.modules))
}

pub fn set(id: ModuleId, on: bool) -> Result<()> {
    config::save_module(&config::path()?, id, on)
}
