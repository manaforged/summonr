use std::path::{Path, PathBuf};

use summonr_agent::hub::{Factory, Info};
use summonr_agent::runtime::Loop;
use summonr_agent::{Client, Error, Requirement, Result};

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
        let ui: Ui = (Some(sender), None);
        let Some(last) = last_chat(&args, &workspace) else {
            return open(&args, &workspace, &ui);
        };
        let mut reopened = args.clone();
        reopened.session = Some(last);
        open(&reopened, &workspace, &ui).or_else(|_| open(&args, &workspace, &ui))
    })
}

/// The agent the window last showed, else the newest chat in this folder, unless the command line chose.
fn last_chat(args: &Args, workspace: &Path) -> Option<String> {
    if args.session.is_some() || args.resume || args.fork || !args.prompt.is_empty() {
        return None;
    }
    let saved = summonr_agent::prefs::Layout::load();
    if let Some(slot) = saved.panes.first().filter(|slot| !slot.session.is_empty()) {
        return Some(slot.session.clone());
    }
    summonr_agent::session::all()
        .ok()?
        .into_iter()
        .filter(|chat| chat.interactive && chat.depth == 0)
        .filter(|chat| {
            chat.cwd
                .as_deref()
                .is_some_and(|cwd| Path::new(cwd) == workspace)
        })
        .map(|chat| chat.id)
        .max()
}

fn open(args: &Args, workspace: &Path, ui: &Ui) -> Result<(Loop<Client>, Info, Factory)> {
    let endpoint = summonr_agent::resolve_endpoint(
        args.endpoint.as_deref(),
        args.model.as_deref(),
        Requirement::Model,
    )?;
    let (root, _, action) = super::super::session::isolate(args, workspace)?;
    std::env::set_current_dir(&root)
        .map_err(|error| Error::Config(format!("Could not enter the project folder: {error}")))?;
    let (loop_, workflow) = assemble(args, &endpoint, &root, action, ui)?;
    let mut info = info(args, &endpoint, &loop_, &root, workflow)?;
    info.splash = false;
    info.inline = false;
    ask_login(ui, &endpoint, args.endpoint.is_some());
    let factory = factory(args, &endpoint, workspace.to_path_buf());
    Ok((loop_, info, factory))
}
