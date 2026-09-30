use std::path::Path;
use std::sync::{Arc, Mutex, Weak};

use serde::Serialize;
use serde_json::Value;

use super::Hub;
use super::catalog;
use super::config::{self, Launch, Server};
use crate::error::Result;

pub type Board = Arc<Mutex<Weak<Hub>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpState {
    Waiting,
    Unlisted,
    Cached,
    Live,
    Stopped,
    Failed,
}

impl McpState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Waiting => "waiting for approval",
            Self::Unlisted => "not listed yet",
            Self::Cached => "cached",
            Self::Live => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    Http,
}

impl McpTransport {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ServerStatus {
    pub name: String,
    pub transport: Option<McpTransport>,
    pub state: McpState,
    pub tools: Vec<String>,
    pub schema_bytes: usize,
    pub launch: Option<String>,
    pub detail: Option<String>,
}

impl ServerStatus {
    #[must_use]
    pub fn schema_size(&self) -> String {
        let tenths = (self.schema_bytes * 10 + 512) / 1024;
        format!("{}.{} KB", tenths / 10, tenths % 10)
    }
}

pub fn status(workspace: &Path, board: Option<&Board>) -> Result<Vec<ServerStatus>> {
    let hub = board.and_then(|board| board.lock().ok()?.upgrade());
    let found = config::servers(workspace)?;
    let mut rows: Vec<ServerStatus> = found
        .servers
        .iter()
        .map(|server| row(server, hub.as_deref()))
        .collect();
    rows.extend(found.waiting.into_iter().map(|name| ServerStatus {
        detail: Some(format!(
            "set `enabled = true` under [mcp.{name}] in ~/.summonr/config.toml to run it"
        )),
        name,
        transport: None,
        state: McpState::Waiting,
        tools: Vec::new(),
        schema_bytes: 0,
        launch: None,
    }));
    Ok(rows)
}

fn row(server: &Server, hub: Option<&Hub>) -> ServerStatus {
    let items = catalog::load(&server.key)
        .map(|listing| listing.tools)
        .unwrap_or_default();
    let advertised: Vec<&Value> = items
        .iter()
        .filter(|item| {
            server.allow.as_ref().is_none_or(|allow| {
                item.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| allow.iter().any(|entry| entry == name))
            })
        })
        .collect();
    let (state, detail) =
        hub.and_then(|hub| hub.state(&server.name))
            .unwrap_or(if items.is_empty() {
                (McpState::Unlisted, None)
            } else {
                (McpState::Cached, None)
            });
    let (transport, launch) = match &server.launch {
        Launch::Stdio { command, args, env } => (
            McpTransport::Stdio,
            format!(
                "{command}{}{}",
                args.iter().map(|arg| format!(" {arg}")).collect::<String>(),
                if env.is_empty() {
                    String::new()
                } else {
                    format!(
                        " (env: {})",
                        env.iter()
                            .map(|(key, _)| key.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            ),
        ),
        Launch::Http { url, .. } => (McpTransport::Http, url.clone()),
    };
    ServerStatus {
        name: server.name.clone(),
        transport: Some(transport),
        state,
        tools: advertised
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect(),
        schema_bytes: advertised
            .iter()
            .map(|item| serde_json::to_vec(item).map_or(0, |bytes| bytes.len()))
            .sum(),
        launch: Some(launch),
        detail,
    }
}
