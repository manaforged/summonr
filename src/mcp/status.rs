use std::path::Path;
use std::sync::{Arc, Mutex, Weak};

use serde::Serialize;
use serde_json::Value;

use super::Hub;
use super::catalog;
use super::config::{self, Launch, Server};
use crate::error::Result;
use crate::supervise::{self, Live, ServerKind};

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
    pub kind: ServerKind,
    pub name: String,
    pub transport: Option<McpTransport>,
    pub state: McpState,
    pub tools: Vec<String>,
    pub schema_bytes: usize,
    pub launch: Option<String>,
    pub detail: Option<String>,
    pub rss_kb: Option<u64>,
}

impl ServerStatus {
    #[must_use]
    pub fn schema_size(&self) -> String {
        let tenths = (self.schema_bytes * 10 + 512) / 1024;
        format!("{}.{} KB", tenths / 10, tenths % 10)
    }

    #[must_use]
    pub fn state_text(&self, width: usize) -> String {
        let text = match (self.state, &self.detail) {
            (McpState::Failed, Some(detail)) => format!("failed: {detail}"),
            (state, _) => state.label().to_string(),
        };
        crate::text::ellipsize(&text, width)
    }
}

pub fn status(workspace: &Path, board: Option<&Board>) -> Result<Vec<ServerStatus>> {
    let hub = board.and_then(|board| board.lock().ok()?.upgrade());
    let found = config::servers(workspace)?;
    let live = supervise::live();
    let file = crate::config::tilde(&crate::config::path()?);
    let mut rows: Vec<ServerStatus> = found
        .servers
        .iter()
        .map(|server| row(server, hub.as_deref(), &live))
        .collect();
    rows.extend(found.waiting.into_iter().map(|name| ServerStatus {
        kind: ServerKind::Mcp,
        detail: Some(format!(
            "set `enabled = true` under [mcp.{name}] in {file} to run it"
        )),
        name,
        transport: None,
        state: McpState::Waiting,
        tools: Vec::new(),
        schema_bytes: 0,
        launch: None,
        rss_kb: None,
    }));
    rows.extend(
        live.iter()
            .filter(|server| server.kind == ServerKind::Lsp)
            .map(|server| ServerStatus {
                kind: ServerKind::Lsp,
                name: server.name.clone(),
                transport: Some(McpTransport::Stdio),
                state: McpState::Live,
                tools: Vec::new(),
                schema_bytes: 0,
                launch: None,
                detail: None,
                rss_kb: server.rss_kb,
            }),
    );
    Ok(rows)
}

fn row(server: &Server, hub: Option<&Hub>, live: &[Live]) -> ServerStatus {
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
    let (state, detail) = hub
        .and_then(|hub| hub.state(&server.name))
        .unwrap_or_else(|| {
            if !items.is_empty() {
                (McpState::Cached, None)
            } else if let Some(error) = catalog::failure(&server.key) {
                (McpState::Failed, Some(error))
            } else {
                (McpState::Unlisted, None)
            }
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
    let rss_kb = (state == McpState::Live)
        .then(|| {
            live.iter()
                .find(|found| found.kind == ServerKind::Mcp && found.name == server.name)
                .and_then(|found| found.rss_kb)
        })
        .flatten();
    ServerStatus {
        kind: ServerKind::Mcp,
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
        rss_kb,
    }
}
