use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use summonr_core::error::{Error, Result};

use file::{Settings, Table};

mod copy;
mod file;
mod ports;

const PREFIXES: [&str; 2] = ["SUMMONR_", "CONDUCTOR_"];

/// A script a repository defines for its workspaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    /// Prepares a new workspace once it exists.
    Setup,
    /// Starts the project in a workspace.
    Run,
    /// Cleans up before a workspace is removed.
    Archive,
}

impl Script {
    /// The script's name as the settings files spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Run => "run",
            Self::Archive => "archive",
        }
    }
}

/// Where a workspace runs: on this machine or on a cloud host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    /// On this machine.
    Local,
    /// On a remote host.
    Cloud,
}

/// Whether run actions may run side by side across a repository's workspaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    /// Actions run at the same time.
    #[default]
    Concurrent,
    /// Starting an action stops every other one first.
    Nonconcurrent,
}

/// One named way to start the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// The action's id; empty for a plain `run` string.
    pub id: String,
    /// The shell command.
    pub command: String,
    /// Arguments appended to the command, each quoted for the shell.
    pub args: Vec<String>,
    /// The folder to run in, relative to the workspace.
    pub cwd: Option<String>,
    /// Whether this is the action to pick first.
    pub default: bool,
    /// The icon name the settings file gives.
    pub icon: Option<String>,
    /// Whether the action is hidden from lists.
    pub hide: bool,
    places: Vec<Place>,
}

impl Action {
    /// What a list shows for the action: `Run`, or `Run <id>`.
    #[must_use]
    pub fn label(&self) -> String {
        let id = self.id.replace('-', " ");
        let id = id.split_whitespace().collect::<Vec<_>>().join(" ");
        if id.is_empty() {
            "Run".to_string()
        } else {
            format!("Run {id}")
        }
    }

    /// Whether the action may run in a workspace at `place`.
    #[must_use]
    pub fn runs_in(&self, place: Place) -> bool {
        self.places.contains(&place)
    }

    fn script(&self) -> String {
        std::iter::once(self.command.clone())
            .chain(self.args.iter().map(|arg| quote(arg)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Env {
    shared: BTreeMap<String, String>,
    local: BTreeMap<String, String>,
    cloud: BTreeMap<String, String>,
}

/// A repository's workspace settings: scripts, run actions, files to copy and environment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scripts {
    setup: Option<String>,
    archive: Option<String>,
    actions: Vec<Action>,
    run_mode: RunMode,
    env: Env,
    include: Vec<String>,
}

impl Scripts {
    /// Reads the scripts of the repository whose primary checkout is `root`.
    pub fn load(root: &Path) -> Result<Self> {
        let table = layered(root)?;
        let parts = table.into_parts();
        Ok(Self {
            include: copy::patterns(root, parts.globs.as_deref())?,
            setup: parts.setup,
            archive: parts.archive,
            actions: parts.actions,
            run_mode: parts.run_mode,
            env: parts.env,
        })
    }

    /// Reads the scripts of the repository that holds the checkout at `path`.
    pub fn at(path: &Path) -> Result<Self> {
        Self::load(&crate::worktree::primary(path).unwrap_or_else(|_| path.to_path_buf()))
    }

    /// The shell command for `script`; for `Run`, the default action's command.
    #[must_use]
    pub fn get(&self, script: Script) -> Option<&str> {
        match script {
            Script::Setup => self.setup.as_deref(),
            Script::Run => self.actions().next().map(|action| action.command.as_str()),
            Script::Archive => self.archive.as_deref(),
        }
    }

    /// The run actions that are not hidden, the default first.
    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        self.actions.iter().filter(|action| !action.hide)
    }

    /// Whether run actions may run side by side.
    #[must_use]
    pub const fn run_mode(&self) -> RunMode {
        self.run_mode
    }

    /// The custom variables for a workspace at `place`, its own table over the shared one.
    #[must_use]
    pub fn env(&self, place: Place) -> BTreeMap<String, String> {
        let own = match place {
            Place::Local => &self.env.local,
            Place::Cloud => &self.env.cloud,
        };
        let mut vars = self.env.shared.clone();
        vars.extend(
            own.iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
        vars
    }

    /// The command that runs `script` in `workspace` with the custom variables.
    #[must_use]
    pub fn command(&self, workspace: &Workspace, script: &str) -> Command {
        let mut command = workspace.command(script);
        command.envs(self.env(workspace.place));
        command
    }

    /// The command that starts `action` in `workspace`; `None` when it does not run there.
    #[must_use]
    pub fn start(&self, workspace: &Workspace, action: &Action) -> Option<Command> {
        if !action.runs_in(workspace.place) {
            return None;
        }
        let mut command = self.command(workspace, &action.script());
        if let Some(cwd) = &action.cwd {
            command.current_dir(workspace.path.join(cwd));
        }
        Some(command)
    }

    /// Copies the root's gitignored files that match the copy patterns into a local workspace.
    pub fn copy_files(&self, workspace: &Workspace) -> Result<Vec<PathBuf>> {
        if workspace.place != Place::Local {
            return Ok(Vec::new());
        }
        copy::copy(&self.include, &workspace.root, &workspace.path)
    }
}

fn layered(root: &Path) -> Result<Table> {
    let own = summonr_core::dirs::workspace(root).join("scripts.toml");
    if let Some(table) = read(&own, toml_text::<Table>)? {
        return Ok(table);
    }
    let conductor = root.join(".conductor");
    let shared = read(&conductor.join("settings.toml"), toml_text::<Settings>)?;
    let local = read(
        &conductor.join("settings.local.toml"),
        toml_text::<Settings>,
    )?;
    if shared.is_some() || local.is_some() {
        let under = Table::from(shared.unwrap_or_default());
        return Ok(Table::from(local.unwrap_or_default()).over(under));
    }
    let legacy = read(&root.join("conductor.json"), json_text::<Settings>)?;
    Ok(legacy.map(Table::from).unwrap_or_default())
}

/// A checkout that scripts run in, with its block of ports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    /// The workspace folder's name.
    pub name: String,
    /// The workspace checkout.
    pub path: PathBuf,
    /// The repository's primary checkout.
    pub root: PathBuf,
    /// The repository's default branch.
    pub default_branch: String,
    /// The first port of the workspace's block.
    pub port: u16,
    /// Where the workspace runs.
    pub place: Place,
}

impl Workspace {
    /// The workspace at `path`, holding the port block that path owns.
    pub fn at(path: &Path) -> Result<Self> {
        let home = summonr_core::dirs::summonr_home()?;
        let root = crate::worktree::primary(path).unwrap_or_else(|_| path.to_path_buf());
        Ok(Self {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.to_path_buf(),
            default_branch: default_branch(&root),
            root,
            port: ports::block(&home, path)?,
            place: Place::Local,
        })
    }

    fn command(&self, script: &str) -> Command {
        let mut command = shell(script);
        command.current_dir(&self.path);
        for (name, value) in self.vars() {
            for prefix in PREFIXES {
                command.env(format!("{prefix}{name}"), &value);
            }
        }
        command
    }

    fn vars(&self) -> Vec<(&'static str, OsString)> {
        let local = self.place == Place::Local;
        let mut vars = vec![
            ("IS_LOCAL", if local { "1" } else { "0" }.into()),
            ("WORKSPACE_NAME", self.name.clone().into()),
            ("WORKSPACE_PATH", self.path.clone().into_os_string()),
            ("ROOT_PATH", self.root.clone().into_os_string()),
            ("DEFAULT_BRANCH", self.default_branch.clone().into()),
        ];
        if local {
            vars.push(("PORT", self.port.to_string().into()));
        }
        vars
    }
}

#[cfg(windows)]
fn quote(arg: &str) -> String {
    format!("\"{}\"", arg.replace('"', "\"\""))
}

#[cfg(not(windows))]
fn quote(arg: &str) -> String {
    summonr_core::shell::quote(arg)
}

#[cfg(windows)]
fn shell(script: &str) -> Command {
    let mut command = Command::new("cmd");
    command.arg("/C").arg(script);
    command
}

#[cfg(not(windows))]
fn shell(script: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(script);
    command
}

fn default_branch(root: &Path) -> String {
    let remote = ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"];
    crate::diff::git_env(root, &[], &remote)
        .ok()
        .and_then(|name| name.split_once('/').map(|(_, branch)| branch.to_string()))
        .or_else(|| crate::diff::git_env(root, &[], &["rev-parse", "--abbrev-ref", "HEAD"]).ok())
        .unwrap_or_default()
}

fn toml_text<T: DeserializeOwned>(text: &str) -> std::result::Result<T, String> {
    toml::from_str(text).map_err(|error| error.to_string())
}

fn json_text<T: DeserializeOwned>(text: &str) -> std::result::Result<T, String> {
    serde_json::from_str(text).map_err(|error| error.to_string())
}

fn read<T>(path: &Path, parse: fn(&str) -> std::result::Result<T, String>) -> Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text)
            .map(Some)
            .map_err(|error| Error::Config(format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
#[path = "scripts_tests.rs"]
mod tests;
