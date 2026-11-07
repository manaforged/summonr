use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use summonr_agent::scripts::{Place, RunMode, Script, Scripts, Workspace};

use super::super::App;
use super::{NEW_TERMINAL, Term, close};

/// The palette action that runs the repository's setup script.
pub(in crate::desktop) const RUN_SETUP: &str = "Run setup";
/// The palette action that runs the repository's default run action.
pub(in crate::desktop) const RUN: &str = "Run";

const SCRIPTS: [(Script, &str); 2] = [(Script::Setup, "Setup"), (Script::Archive, "Archive")];

/// Runs the repository's `script` in a new terminal for `pane`'s folder, showing it there when `show`.
pub(in crate::desktop) fn run_script(pane: &App, script: Script, show: bool) {
    if script == Script::Run {
        return run_action(pane, None);
    }
    let folder = pane.workspace();
    let name = SCRIPTS
        .iter()
        .find(|(each, _)| *each == script)
        .map_or(NEW_TERMINAL, |(_, name)| name);
    let started = script_command(&folder, script).and_then(|command| {
        command
            .map(|command| {
                pane.window
                    .terminals
                    .start(&folder, name, Some(&command), None)
            })
            .transpose()
    });
    match started {
        Ok(Some(term)) => {
            if show {
                pane.window.showing.set(Some(term.id));
            }
        }
        Ok(None) => {
            if show {
                say(
                    pane,
                    format!("This repository has no {} script.", script.name()),
                );
            }
        }
        Err(error) => say(
            pane,
            format!("Could not run the {} script: {error}", script.name()),
        ),
    }
}

fn script_command(folder: &Path, script: Script) -> Result<Option<Command>, String> {
    let workspace = Workspace::at(folder).map_err(|error| error.to_string())?;
    let scripts = Scripts::load(&workspace.root).map_err(|error| error.to_string())?;
    if script == Script::Setup {
        scripts
            .copy_files(&workspace)
            .map_err(|error| error.to_string())?;
    }
    Ok(scripts
        .get(script)
        .map(|text| scripts.command(&workspace, text)))
}

/// A run action as the palette lists it.
pub(in crate::desktop) struct RunRow {
    /// `Run` or `Run <id>`.
    pub(in crate::desktop) label: String,
    /// The action's id; `None` for a plain `run` string.
    pub(in crate::desktop) id: Option<String>,
    /// Whether the action runs only in the cloud.
    pub(in crate::desktop) cloud_only: bool,
}

/// The run actions the palette lists for `folder`, the default first.
pub(in crate::desktop) fn run_rows(folder: &Path) -> Vec<RunRow> {
    Scripts::at(folder).map_or_else(
        |_| Vec::new(),
        |scripts| {
            scripts
                .actions()
                .map(|action| RunRow {
                    label: action.label(),
                    id: (!action.id.is_empty()).then(|| action.id.clone()),
                    cloud_only: !action.runs_in(Place::Local),
                })
                .collect()
        },
    )
}

/// Starts run action `id` (the default when `None`) in a terminal for `pane`'s folder and shows it.
pub(in crate::desktop) fn run_action(pane: &App, id: Option<&str>) {
    match start_action(pane, id) {
        Ok(term) => pane.window.showing.set(Some(term.id)),
        Err(error) => say(pane, error),
    }
}

fn start_action(pane: &App, id: Option<&str>) -> Result<Rc<Term>, String> {
    let folder = pane.workspace();
    let workspace = Workspace::at(&folder).map_err(|error| error.to_string())?;
    let scripts = Scripts::load(&workspace.root).map_err(|error| error.to_string())?;
    let mut actions = scripts.actions();
    let action = match id {
        Some(id) => actions.find(|action| action.id == id),
        None => actions.next(),
    }
    .ok_or_else(|| "This repository has no run script.".to_owned())?;
    let label = action.label();
    let command = scripts
        .start(&workspace, action)
        .ok_or_else(|| format!("{label} is cloud only; it does not run here."))?;
    if scripts.run_mode() == RunMode::Nonconcurrent {
        stop_runs(pane, &workspace.root);
    }
    pane.window
        .terminals
        .start(&folder, &label, Some(&command), Some(workspace.root))
        .map_err(|error| format!("Could not start {label}: {error}"))
}

fn stop_runs(app: &App, root: &Path) {
    let running: Vec<u64> = app.window.terminals.list.with(|list| {
        list.iter()
            .filter(|term| term.run.as_deref() == Some(root))
            .map(|term| term.id)
            .collect()
    });
    for id in running {
        close(app, id);
    }
}

fn say(pane: &App, text: String) {
    pane.push_entry(crate::model::Entry::Error(text));
}
