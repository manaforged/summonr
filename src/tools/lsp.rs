use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{Value, json};
use url::Url;

use crate::error::{Error, Result};
use crate::language;
use crate::process::Env;
use crate::rpc::{Client, Framing, Peer, Spawn};

use super::{ToolOutput, ToolRegistry, nonempty_path, object_bool, object_usize};

pub(super) struct Conn {
    client: Client,
    docs: std::collections::HashMap<String, (String, i64, (u64, u64))>,
    languages: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    Host,
    Plugin,
}

#[derive(Debug)]
pub(super) struct Launch {
    origin: Origin,
    key: String,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    languages: std::collections::BTreeMap<String, String>,
    options: Value,
}

impl Conn {
    pub(super) async fn spawn(workspace: &Path, launch: &Launch) -> Result<Self> {
        let (program, args) = launch
            .argv
            .split_first()
            .ok_or_else(|| Error::Tool("language server has no command".to_string()))?;
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let root = file_uri(workspace)?;
        let limits = crate::limits::limits();
        let client = Client::spawn(&Spawn {
            program,
            args: &args,
            env: &launch.env,
            cwd: workspace,
            wait: limits.lsp_start(),
            base: match launch.origin {
                Origin::Host => Env::Inherit,
                Origin::Plugin => Env::Clean,
            },
            framing: Framing::ContentLength,
            max: limits.lsp_frame_max_bytes,
            peer: Peer::Lsp(crate::rpc::Lsp {
                root: root.clone(),
                options: launch.options.clone(),
                sections: vec![launch.argv[0].clone(), launch.key.clone()],
            }),
        })?;
        let mut conn = Self {
            client,
            docs: std::collections::HashMap::new(),
            languages: launch.languages.clone(),
        };
        conn.handshake(&root, &launch.options).await?;
        Ok(conn)
    }

    pub(super) fn pid(&self) -> Option<u32> {
        self.client.pid()
    }

    pub(super) fn set_wait(&mut self, wait: std::time::Duration) {
        self.client.set_wait(wait);
    }

    pub(super) fn alive(&mut self) -> bool {
        self.client.alive()
    }

    async fn handshake(&mut self, root: &str, options: &Value) -> Result<()> {
        let result = self
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root,
                    "workspaceFolders": [{ "uri": root, "name": "workspace" }],
                    "initializationOptions": options,
                    "capabilities": {
                        "workspace": { "workspaceFolders": true, "configuration": true },
                        "window": { "workDoneProgress": true },
                        "textDocument": {
                            "publishDiagnostics": {},
                            "definition": {},
                            "references": {},
                            "rename": {},
                            "diagnostic": {}
                        }
                    }
                }),
            )
            .await?;
        drop(result);
        self.notify("initialized", json!({})).await?;
        Ok(())
    }

    pub(super) async fn diagnostics(&mut self, path: &Path) -> Result<(String, bool)> {
        self.did_open(path).await?;
        let uri = file_uri(path)?;
        let result = self
            .request(
                "textDocument/diagnostic",
                json!({ "textDocument": { "uri": uri } }),
            )
            .await?;
        let items = diagnostic_items(&result);
        if items.is_empty() {
            return Ok(("no diagnostics".to_string(), false));
        }
        Ok((items.join("\n"), has_error(&result)))
    }

    pub(super) async fn definition(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<String> {
        let result = self
            .at(path, line, character, "textDocument/definition")
            .await?;
        let locations = locations(&result);
        if locations.is_empty() {
            return Ok("no definition".to_string());
        }
        Ok(locations.join("\n"))
    }

    pub(super) async fn references(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<String> {
        self.did_open(path).await?;
        let uri = file_uri(path)?;
        let line = line.saturating_sub(1);
        let character = character.saturating_sub(1);
        let result = self
            .request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                    "context": { "includeDeclaration": true }
                }),
            )
            .await?;
        let locations = locations(&result);
        if locations.is_empty() {
            return Ok("no references".to_string());
        }
        Ok(locations.join("\n"))
    }

    pub(super) async fn rename(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<Value> {
        self.did_open(path).await?;
        let uri = file_uri(path)?;
        let line = line.saturating_sub(1);
        let character = character.saturating_sub(1);
        self.request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "newName": new_name
            }),
        )
        .await
    }

    async fn at(&mut self, path: &Path, line: u32, character: u32, method: &str) -> Result<Value> {
        self.did_open(path).await?;
        let uri = file_uri(path)?;
        let line = line.saturating_sub(1);
        let character = character.saturating_sub(1);
        self.request(
            method,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
        .await
    }

    async fn did_open(&mut self, path: &Path) -> Result<()> {
        docs::did_open(self, path).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.client.call(method, params, |note| drop(note)).await
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.client.notify(method, params).await
    }
}

mod detect;
mod docs;
mod proto;
mod tool;

use detect::Scope;
pub(in crate::tools) use detect::{Launches, available};
use proto::*;
