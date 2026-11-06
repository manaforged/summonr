use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::config;
use super::conn::Conn;
use super::link::Link;
use super::tool::offered;
use crate::supervise::Slot;
use summonr_core::error::Result;

pub async fn probe(workspace: &Path, name: &str) -> Result<Option<usize>> {
    let Some(server) = config::servers(workspace)?
        .servers
        .into_iter()
        .find(|server| server.name == name)
    else {
        return Ok(None);
    };
    let slot: Slot<Conn> = Arc::new(Mutex::new(None));
    let listed = Link::list(&server, workspace, &slot).await;
    *slot.lock().await = None;
    listed.map(|listing| Some(offered(&server, &listing.tools).count()))
}
