//! Curated language policy shared by the harness tools.

use std::path::Path;

use serde_json::{Value, json};

use crate::error::{Error, Result};

#[derive(Debug)]
/// One compiled-in language policy.
pub(crate) struct Module {
    pub(crate) id: &'static str,
    pub(crate) markers: &'static [&'static str],
    pub(crate) extensions: &'static [&'static str],
    pub(crate) servers: &'static [Server],
    pub(crate) commands: &'static [&'static str],
}

#[derive(Debug)]
/// One language-server command supplied by a module.
pub(crate) struct Server {
    pub(crate) name: &'static str,
    pub(crate) args: &'static [&'static str],
}

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
    },
    Module {
        id: "typescript",
        markers: &["package.json"],
        extensions: &["js", "jsx", "mjs", "cjs", "ts", "tsx"],
        servers: &[
            Server {
                name: "biome",
                args: &["lsp-proxy"],
            },
            Server {
                name: "typescript-language-server",
                args: &["--stdio"],
            },
        ],
        commands: &["npm", "pnpm", "yarn", "bun", "node --test", "tsc"],
    },
    Module {
        id: "python",
        markers: &["pyproject.toml", "setup.py"],
        extensions: &["py"],
        servers: &[
            Server {
                name: "ruff",
                args: &["server"],
            },
            Server {
                name: "pyright-langserver",
                args: &["--stdio"],
            },
        ],
        commands: &["pytest", "python -m pytest", "ruff", "mypy", "pyright"],
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
    },
];

/// Returns a module selected by user configuration.
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

/// Selects a module from the file extension and nearest project marker.
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

/// Returns initialization options for the module's language server.
pub(crate) fn options(module: &Module) -> Value {
    match module.id {
        "rust" => json!({
            "checkOnSave": false,
            "cargo": {
                "allTargets": false,
                "buildScripts": { "enable": false }
            },
            "procMacro": { "enable": false }
        }),
        _ => json!({}),
    }
}

/// Returns the LSP language identifier for a file.
pub(crate) fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => "rust",
        Some("go") => "go",
        Some("ts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("jsx") => "javascriptreact",
        Some("py") => "python",
        Some("toml") => "toml",
        _ => "plaintext",
    }
}

/// Reports whether a command belongs to a compiled-in toolchain.
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
