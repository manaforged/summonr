use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::catalog;
use super::config;
use super::conn::Conn;
use super::link::Link;
use super::tool::Tool;
use crate::error::{Error, Result};
use crate::supervise::Slot;

pub async fn probe(workspace: &Path, name: &str) -> Result<usize> {
    let server = config::servers(workspace)?
        .servers
        .into_iter()
        .find(|server| server.name == name)
        .ok_or_else(|| Error::Config(format!("no MCP server named {name}")))?;
    let slot: Slot<Conn> = Arc::new(Mutex::new(None));
    let listed = Link::list(&server, workspace, None, &slot).await;
    *slot.lock().await = None;
    match listed {
        Ok(listing) => Ok(listing
            .tools
            .iter()
            .filter_map(|item| Tool::parse(&server.name, item))
            .filter(|tool| {
                server
                    .allow
                    .as_ref()
                    .is_none_or(|allow| allow.contains(&tool.name))
            })
            .count()),
        Err(error) => {
            catalog::fail(&server.key, &error.to_string());
            Err(error)
        }
    }
}
