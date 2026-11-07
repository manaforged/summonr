use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde::de::{DeserializeOwned, IgnoredAny};
use summonr_core::error::{Error, Result};

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

/// The shell commands a repository defines for setup, run and archive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scripts {
    setup: Option<String>,
    run: Option<String>,
    archive: Option<String>,
}

impl Scripts {
    /// Reads the scripts of the repository whose primary checkout is `root`.
    pub fn load(root: &Path) -> Result<Self> {
        let own = root.join(".summonr").join("scripts.toml");
        if let Some(table) = read(&own, toml_text::<Table>)? {
            return Ok(table.into());
        }
        let conductor = root.join(".conductor");
        let shared = read(&conductor.join("settings.toml"), toml_text::<Settings>)?;
        let local = read(
            &conductor.join("settings.local.toml"),
            toml_text::<Settings>,
        )?;
        if shared.is_some() || local.is_some() {
            let under = Self::from(shared.unwrap_or_default().scripts);
            return Ok(Self::from(local.unwrap_or_default().scripts).over(under));
        }
        let legacy = read(&root.join("conductor.json"), json_text::<Settings>)?;
        Ok(legacy.map(|file| file.scripts.into()).unwrap_or_default())
    }

    /// The shell command for `script`, when the repository defines one.
    #[must_use]
    pub fn get(&self, script: Script) -> Option<&str> {
        match script {
            Script::Setup => self.setup.as_deref(),
            Script::Run => self.run.as_deref(),
            Script::Archive => self.archive.as_deref(),
        }
    }

    fn over(self, under: Self) -> Self {
        Self {
            setup: self.setup.or(under.setup),
            run: self.run.or(under.run),
            archive: self.archive.or(under.archive),
        }
    }
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
        })
    }

    /// The command that runs `script` through the shell in this workspace.
    #[must_use]
    pub fn command(&self, script: &str) -> Command {
        let mut command = shell(script);
        command.current_dir(&self.path);
        for (name, value) in self.vars() {
            for prefix in PREFIXES {
                command.env(format!("{prefix}{name}"), &value);
            }
        }
        command
    }

    fn vars(&self) -> [(&'static str, OsString); 5] {
        [
            ("PORT", self.port.to_string().into()),
            ("WORKSPACE_NAME", self.name.clone().into()),
            ("WORKSPACE_PATH", self.path.clone().into_os_string()),
            ("ROOT_PATH", self.root.clone().into_os_string()),
            ("DEFAULT_BRANCH", self.default_branch.clone().into()),
        ]
    }
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

#[derive(Default, Deserialize)]
struct Table {
    setup: Option<Field>,
    run: Option<Field>,
    archive: Option<Field>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Field {
    Text(String),
    Other(IgnoredAny),
}

impl Field {
    fn text(self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text),
            Self::Other(_) => None,
        }
    }
}

impl From<Table> for Scripts {
    fn from(table: Table) -> Self {
        Self {
            setup: table.setup.and_then(Field::text),
            run: table.run.and_then(Field::text),
            archive: table.archive.and_then(Field::text),
        }
    }
}

#[derive(Default, Deserialize)]
struct Settings {
    #[serde(default)]
    scripts: Table,
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
