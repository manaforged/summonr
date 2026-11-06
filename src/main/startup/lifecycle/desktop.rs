use std::path::PathBuf;

use summonr_agent::{Error, Requirement, Result};

use super::{Ui, ask_login, assemble, factory, info};
use crate::entry::cli::Args;

pub(in crate::entry::startup) fn run(args: &Args) -> Result<i32> {
    let args = args.clone();
    summonr_agent::mark_interactive();
    summonr_ui::desktop(move |workspace: PathBuf, sender| {
        let workspace = workspace.canonicalize().map_err(|error| {
            Error::Config(format!("Could not open the project folder: {error}"))
        })?;
        if !workspace.is_dir() {
            return Err(Error::Config(
                "Choose a project folder, not a file.".to_string(),
            ));
        }
        summonr_agent::check_config()?;
        super::super::commands::adopt(&args)?;
        let endpoint = summonr_agent::resolve_endpoint(
            args.endpoint.as_deref(),
            args.model.as_deref(),
            Requirement::Model,
        )?;
        let ui: Ui = (Some(sender), None);
        let (root, _, action) = super::super::session::isolate(&args, &workspace)?;
        std::env::set_current_dir(&root).map_err(|error| {
            Error::Config(format!("Could not enter the project folder: {error}"))
        })?;
        let (loop_, _, workflow) = assemble(&args, &endpoint, &root, action, &ui)?;
        let mut info = info(&args, &endpoint, &loop_, &root, workflow)?;
        info.splash = false;
        info.inline = false;
        ask_login(&ui, &endpoint, args.endpoint.is_some());
        let factory = factory(&args, &endpoint, workspace);
        Ok((loop_, info, factory))
    })
}
