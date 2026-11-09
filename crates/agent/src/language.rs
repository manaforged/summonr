use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

use serde::Deserialize;
use serde::de::IntoDeserializer;
use serde_json::Value;

use summonr_core::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Language {
    Rust,
    Typescript,
    Python,
    Go,
    Lean,
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Rust => "rust",
            Self::Typescript => "typescript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Lean => "lean",
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Module {
    pub(crate) id: Language,
    pub(crate) markers: Vec<String>,
    #[serde(default)]
    pub(crate) root_at_marker: bool,
    pub(crate) servers: Vec<Server>,
    pub(crate) commands: Vec<String>,
    pub(crate) options: Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Server {
    pub(crate) name: String,
    pub(crate) args: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Extension {
    name: String,
    language_id: String,
    module: Option<Language>,
}

#[derive(Debug, Deserialize)]
struct Table {
    module: Vec<Module>,
    extension: Vec<Extension>,
}

const PLAIN_TEXT: &str = "plaintext";

static TABLE: LazyLock<Table> =
    LazyLock::new(|| summonr_core::embedded!("language", "language.toml"));

impl Module {
    pub(crate) fn handles(&self, path: &Path) -> bool {
        extension(path).and_then(|row| row.module) == Some(self.id)
    }

    pub(crate) fn project_dir<'a>(&self, workspace: &Path, path: &'a Path) -> Option<&'a Path> {
        project_dir(workspace, path, self)
    }
}

fn extension(path: &Path) -> Option<&'static Extension> {
    let name = path.extension().and_then(|extension| extension.to_str())?;
    TABLE.extension.iter().find(|row| row.name == name)
}

pub(crate) fn named(name: &str) -> Result<&'static Module> {
    let parsed: std::result::Result<Language, serde::de::value::Error> =
        Language::deserialize(name.into_deserializer());
    parsed
        .ok()
        .and_then(|id| TABLE.module.iter().find(|module| module.id == id))
        .ok_or_else(|| {
            Error::Config(format!(
                "unknown language module `{name}`; use rust, typescript, python, go, or lean"
            ))
        })
}

pub(crate) fn detect(workspace: &Path, path: &Path) -> Result<&'static Module> {
    let id = extension(path).and_then(|row| row.module);
    let module = TABLE
        .module
        .iter()
        .find(|module| Some(module.id) == id)
        .ok_or_else(|| {
            Error::Tool(format!(
                "no language module for `{}`; supported modules are rust, typescript, python, go, and lean",
                path.display()
            ))
        })?;
    if module.project_dir(workspace, path).is_some() {
        return Ok(module);
    }
    Err(Error::Tool(format!(
        "{} module needs one of {} between the file and workspace root",
        module.id,
        module.markers.join(", ")
    )))
}

pub(crate) fn present(workspace: &Path) -> Vec<&'static Module> {
    let mut roots = vec![workspace.to_path_buf()];
    roots.extend(
        summonr_core::walk::children(workspace)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir()),
    );
    TABLE
        .module
        .iter()
        .filter(|module| roots.iter().any(|root| has_marker(root, module)))
        .collect()
}

pub(crate) fn options(module: &Module) -> Value {
    module.options.clone()
}

pub(crate) fn language_id(path: &Path) -> &'static str {
    extension(path).map_or(PLAIN_TEXT, |row| row.language_id.as_str())
}

pub(crate) fn toolchain(command: &str) -> bool {
    TABLE
        .module
        .iter()
        .flat_map(|module| &module.commands)
        // complexity: bounded by the build commands in language.toml
        .any(|tool| command.contains(tool.as_str()))
}

fn has_marker(directory: &Path, module: &Module) -> bool {
    module
        .markers
        .iter()
        .any(|marker| directory.join(marker).is_file())
}

fn project_dir<'a>(workspace: &Path, path: &'a Path, module: &Module) -> Option<&'a Path> {
    let Some(start) = path.parent() else {
        return None;
    };
    for directory in start.ancestors() {
        if !directory.starts_with(workspace) {
            break;
        }
        if has_marker(directory, module) {
            return Some(directory);
        }
        if directory == workspace {
            break;
        }
    }
    None
}
