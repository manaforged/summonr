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
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Rust => "rust",
            Self::Typescript => "typescript",
            Self::Python => "python",
            Self::Go => "go",
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Module {
    pub(crate) id: Language,
    pub(crate) markers: Vec<String>,
    pub(crate) servers: Vec<Server>,
    pub(crate) commands: Vec<String>,
    pub(crate) options: Value,
    pub(crate) check: Option<Check>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Check {
    pub(crate) command: String,
    pub(crate) requires: Option<Requirement>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Requirement {
    pub(crate) file: String,
    pub(crate) pointer: String,
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
                "unknown language module `{name}`; use rust, typescript, python, or go"
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
                "no language module for `{}`; supported modules are rust, typescript, python, and go",
                path.display()
            ))
        })?;
    if marked(workspace, path, module) {
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
        // complexity: bounded by the four modules in language.toml
        .filter(|module| roots.iter().any(|root| has_marker(root, module)))
        .collect()
}

#[must_use]
pub(crate) fn check(workspace: &Path) -> Option<&'static str> {
    TABLE
        .module
        .iter()
        .filter(|module| has_marker(workspace, module))
        .filter_map(|module| module.check.as_ref())
        .find(|check| {
            check.requires.as_ref().is_none_or(|requirement| {
                std::fs::read_to_string(workspace.join(&requirement.file))
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .and_then(|value| value.pointer(&requirement.pointer).cloned())
                    .is_some_and(|script| {
                        script.as_str().is_some_and(|text| !text.trim().is_empty())
                    })
            })
        })
        .map(|check| check.command.as_str())
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

fn marked(workspace: &Path, path: &Path, module: &Module) -> bool {
    let Some(start) = path.parent() else {
        return false;
    };
    for directory in start.ancestors() {
        if !directory.starts_with(workspace) {
            break;
        }
        if has_marker(directory, module) {
            return true;
        }
        if directory == workspace {
            break;
        }
    }
    false
}

#[cfg(test)]
#[path = "language_tests.rs"]
mod tests;
