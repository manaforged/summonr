use std::path::Path;

use serde_json::{Value, json};

use crate::error::{Error, Result};

#[derive(Debug)]
pub(crate) struct Module {
    pub(crate) id: &'static str,
    pub(crate) markers: &'static [&'static str],
    pub(crate) extensions: &'static [&'static str],
    pub(crate) servers: &'static [Server],
    pub(crate) commands: &'static [&'static str],
    pub(crate) options: &'static str,
    pub(crate) check: Option<Check>,
}

#[derive(Debug)]
pub(crate) struct Check {
    pub(crate) command: &'static str,
    pub(crate) requires: Option<Requirement>,
}

#[derive(Debug)]
pub(crate) struct Requirement {
    pub(crate) file: &'static str,
    pub(crate) pointer: &'static str,
}

#[derive(Debug)]
pub(crate) struct Server {
    pub(crate) name: &'static str,
    pub(crate) args: &'static [&'static str],
}

const PLAIN_TEXT: &str = "plaintext";

const LANGUAGE_IDS: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("go", "go"),
    ("ts", "typescript"),
    ("tsx", "typescriptreact"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascriptreact"),
    ("py", "python"),
    ("toml", "toml"),
];

const MODULES: &[Module] = &[
    Module {
        id: "rust",
        markers: &["Cargo.toml"],
        extensions: &["rs"],
        servers: &[Server {
            name: "rust-analyzer",
            args: &[],
        }],
        commands: &["cargo", "rustc"],
        options: r#"{"checkOnSave": false, "cargo": {"allTargets": false, "buildScripts": {"enable": false}}, "procMacro": {"enable": false}}"#,
        check: Some(Check {
            command: "cargo test",
            requires: None,
        }),
    },
    Module {
        id: "typescript",
        markers: &["package.json"],
        extensions: &["js", "jsx", "mjs", "cjs", "ts", "tsx"],
        servers: &[
            Server {
                name: "typescript-language-server",
                args: &["--stdio"],
            },
            Server {
                name: "biome",
                args: &["lsp-proxy"],
            },
        ],
        commands: &["npm", "pnpm", "yarn", "bun", "node --test", "tsc"],
        options: "{}",
        check: Some(Check {
            command: "npm test",
            requires: Some(Requirement {
                file: "package.json",
                pointer: "/scripts/test",
            }),
        }),
    },
    Module {
        id: "python",
        markers: &["pyproject.toml", "setup.py"],
        extensions: &["py"],
        servers: &[
            Server {
                name: "pyright-langserver",
                args: &["--stdio"],
            },
            Server {
                name: "ruff",
                args: &["server"],
            },
        ],
        commands: &["pytest", "python -m pytest", "ruff", "mypy", "pyright"],
        options: "{}",
        check: None,
    },
    Module {
        id: "go",
        markers: &["go.mod"],
        extensions: &["go"],
        servers: &[Server {
            name: "gopls",
            args: &[],
        }],
        commands: &["go test", "go build", "go vet"],
        options: "{}",
        check: Some(Check {
            command: "go test ./...",
            requires: None,
        }),
    },
];

pub(crate) fn named(name: &str) -> Result<&'static Module> {
    MODULES
        .iter()
        .find(|module| module.id == name)
        .ok_or_else(|| {
            Error::Config(format!(
                "unknown language module `{name}`; use rust, typescript, python, or go"
            ))
        })
}

pub(crate) fn detect(workspace: &Path, path: &Path) -> Result<&'static Module> {
    let extension = path.extension().and_then(|extension| extension.to_str());
    let module = MODULES
        .iter()
        .find(|module| extension.is_some_and(|value| module.extensions.contains(&value)))
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
    if let Ok(entries) = std::fs::read_dir(workspace) {
        roots.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir()),
        );
    }
    MODULES
        .iter()
        .filter(|module| {
            roots.iter().any(|root| {
                module
                    .markers
                    .iter()
                    .any(|marker| root.join(marker).is_file())
            })
        })
        .collect()
}

#[must_use]
pub(crate) fn check(workspace: &Path) -> Option<&'static str> {
    MODULES
        .iter()
        .filter(|module| {
            module
                .markers
                .iter()
                .any(|marker| workspace.join(marker).is_file())
        })
        .filter_map(|module| module.check.as_ref())
        .find(|check| {
            check.requires.as_ref().is_none_or(|requirement| {
                std::fs::read_to_string(workspace.join(requirement.file))
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .and_then(|value| value.pointer(requirement.pointer).cloned())
                    .is_some_and(|script| {
                        script.as_str().is_some_and(|text| !text.trim().is_empty())
                    })
            })
        })
        .map(|check| check.command)
}

pub(crate) fn options(module: &Module) -> Value {
    serde_json::from_str(module.options).unwrap_or_else(|_error| json!({}))
}

pub(crate) fn language_id(path: &Path) -> &'static str {
    let extension = path.extension().and_then(|extension| extension.to_str());
    LANGUAGE_IDS
        .iter()
        .find(|(known, _)| extension == Some(*known))
        .map_or(PLAIN_TEXT, |(_, id)| id)
}

pub(crate) fn toolchain(command: &str) -> bool {
    MODULES
        .iter()
        .flat_map(|module| module.commands)
        .any(|tool| command.contains(tool))
}

fn marked(workspace: &Path, path: &Path, module: &Module) -> bool {
    let Some(start) = path.parent() else {
        return false;
    };
    for directory in start.ancestors() {
        if !directory.starts_with(workspace) {
            break;
        }
        if module
            .markers
            .iter()
            .any(|marker| directory.join(marker).is_file())
        {
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
