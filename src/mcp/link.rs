use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::policy::Policy;
use crate::rpc::Notice;
use crate::supervise::{self, ServerKind, Slot};

use super::catalog::{self, Listing};
use super::config::Server;
use super::conn::Conn;
use super::form::Form;
use super::journal::Journal;
use super::status::McpState;
use super::tool::Tool;
use super::{Answer, McpProgress, McpUpdate};

pub(super) struct Link {
    server: Server,
    workspace: PathBuf,
    conn: Slot<Conn>,
    fresh: std::sync::Mutex<Option<HashSet<String>>>,
    started: AtomicBool,
    detail: std::sync::Mutex<Option<String>>,
    journal: Journal,
    pub(super) tools: Vec<Tool>,
}

impl Link {
    pub(super) async fn open(server: Server, workspace: PathBuf) -> Result<Arc<Self>> {
        let slot: Slot<Conn> = Arc::new(Mutex::new(None));
        let listing = match catalog::load(&server.key) {
            Some(listing) => listing,
            None => Self::list(&server, &workspace, &slot).await?,
        };
        let started = AtomicBool::new(slot.lock().await.is_some());
        let tools = listing
            .tools
            .iter()
            .filter_map(|item| Tool::parse(&server.name, item))
            .filter(|tool| {
                server
                    .allow
                    .as_ref()
                    // complexity: bounded by a BTreeSet lookup, O(log n)
                    .is_none_or(|allow| allow.contains(&tool.name))
            })
            .collect();
        Ok(Arc::new(Self {
            server,
            workspace,
            conn: slot,
            fresh: std::sync::Mutex::new(None),
            started,
            detail: std::sync::Mutex::new(None),
            journal: Journal::default(),
            tools,
        }))
    }

    pub(super) async fn list(
        server: &Server,
        workspace: &std::path::Path,
        slot: &Slot<Conn>,
    ) -> Result<Listing> {
        let mut held = slot.lock().await;
        let conn = held.insert(Conn::open(server, workspace).await?);
        supervise::register(ServerKind::Mcp, &server.name, conn.pid(), server.idle, slot);
        let listing = conn.listing().await?;
        catalog::save(&server.key, &listing);
        conn.ready(server.wait)?;
        if server.idle.is_some() {
            *held = None;
        }
        Ok(listing)
    }

    pub(super) async fn call_tool(
        self: &Arc<Self>,
        tool: &Tool,
        arguments: Value,
        gate: &Gate<'_>,
        report: &mut (dyn FnMut(McpUpdate) + Send),
    ) -> Result<Called> {
        gate.policy.tool(&tool.alias, false)?;
        let mut slot = self.conn.lock().await;
        self.ensure(&mut slot).await?;
        self.offered(tool)?;
        let params = json!({"name": tool.name, "arguments": arguments});
        let (value, changed) = self
            .exchange(&mut slot, "tools/call", params, report)
            .await?;
        Ok(Called { value, changed })
    }

    pub(super) async fn request(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        report: &mut (dyn FnMut(McpUpdate) + Send),
    ) -> Result<Value> {
        let mut slot = self.conn.lock().await;
        self.ensure(&mut slot).await?;
        self.exchange(&mut slot, method, params, report)
            .await
            .map(|(value, _)| value)
    }

    async fn exchange(
        self: &Arc<Self>,
        slot: &mut Option<Conn>,
        method: &str,
        params: Value,
        report: &mut (dyn FnMut(McpUpdate) + Send),
    ) -> Result<(Value, bool)> {
        let conn = slot
            .as_mut()
            .ok_or_else(|| Error::Tool(format!("mcp {} is not connected", self.server.name)))?;
        let mut changes = Changes::default();
        let pending = Pending(Some(Arc::downgrade(self)));
        let result = conn
            .call(method, params, &mut |notice| {
                self.hear(notice, &mut changes, report);
            })
            .await;
        pending.settle();
        self.after(slot, result.as_ref().err(), changes).await;
        supervise::touch(&self.conn);
        result.map(|value| (value, changes.tools))
    }

    pub(super) fn listing(&self) -> Listing {
        catalog::load(&self.server.key).unwrap_or_default()
    }

    async fn ensure(self: &Arc<Self>, slot: &mut Option<Conn>) -> Result<()> {
        if !slot.as_mut().is_none_or(|conn| !conn.alive()) {
            return Ok(());
        }
        *slot = None;
        let mut conn = Conn::open(&self.server, &self.workspace)
            .await
            .inspect_err(|error| self.note(error))?;
        self.started.store(true, Ordering::Relaxed);
        if let Ok(listing) = conn.listing().await {
            catalog::save(&self.server.key, &listing);
            self.remember(&listing.tools);
        }
        conn.ready(self.server.wait)?;
        supervise::register(
            ServerKind::Mcp,
            &self.server.name,
            conn.pid(),
            self.server.idle,
            &self.conn,
        );
        *slot = Some(conn);
        Ok(())
    }

    fn hear(
        &self,
        notice: Notice<'_>,
        changes: &mut Changes,
        report: &mut (dyn FnMut(McpUpdate) + Send),
    ) {
        match notice {
            Notice::ToolsChanged => changes.tools = true,
            Notice::PromptsChanged | Notice::ResourcesChanged => changes.other = true,
            Notice::Progress {
                progress,
                total,
                message,
                ..
            } => report(McpUpdate::Progress(McpProgress {
                progress,
                total,
                message: message.map(str::to_string),
            })),
            Notice::Message {
                level,
                logger,
                data,
            } => self.journal.append(&self.server.name, level, logger, data),
            Notice::Elicit { params, answer } => match Form::parse(&self.server.name, params) {
                Some(form) => report(McpUpdate::Elicit {
                    form,
                    answer: Answer(answer),
                }),
                None => drop(answer.send(crate::rpc::Reply::Error(
                    -32602,
                    "summonr supports form elicitation with a flat schema".to_string(),
                ))),
            },
            Notice::Unknown => {}
        }
    }

    async fn after(&self, slot: &mut Option<Conn>, failed: Option<&Error>, changes: Changes) {
        let Some(conn) = slot.as_mut() else {
            return;
        };
        if let Some(error) = failed
            && !conn.alive()
        {
            self.note(error);
            *slot = None;
        } else if (changes.tools || changes.other)
            && let Ok(listing) = conn.listing().await
        {
            catalog::save(&self.server.key, &listing);
            self.remember(&listing.tools);
        }
    }

    pub(super) fn name(&self) -> &str {
        &self.server.name
    }

    pub(super) fn state(&self) -> (McpState, Option<String>) {
        let live = self.conn.try_lock().map_or(true, |slot| slot.is_some());
        let state = if live {
            McpState::Live
        } else if self.started.load(Ordering::Relaxed) {
            McpState::Stopped
        } else {
            McpState::Cached
        };
        (
            state,
            self.detail.lock().ok().and_then(|detail| detail.clone()),
        )
    }

    fn note(&self, error: &Error) {
        if let Ok(mut detail) = self.detail.lock() {
            *detail = Some(error.to_string());
        }
    }

    fn remember(&self, items: &[Value]) {
        let names = items
            .iter()
            .filter_map(|item| Tool::parse(&self.server.name, item))
            .map(|tool| tool.name)
            .collect();
        if let Ok(mut fresh) = self.fresh.lock() {
            *fresh = Some(names);
        }
    }

    fn offered(&self, tool: &Tool) -> Result<()> {
        let fresh = self.fresh.lock().ok();
        let Some(names) = fresh.as_ref().and_then(|fresh| fresh.as_ref()) else {
            return Ok(());
        };
        if names.contains(&tool.name) {
            return Ok(());
        }
        Err(Error::Tool(format!(
            "`{}` is no longer offered by mcp {}",
            tool.alias, self.server.name
        )))
    }
}

pub(super) struct Gate<'a> {
    pub(super) policy: &'a Policy,
}

#[derive(Clone, Copy, Default)]
struct Changes {
    tools: bool,
    other: bool,
}

pub(super) struct Called {
    pub(super) value: Value,
    pub(super) changed: bool,
}

struct Pending(Option<Weak<Link>>);

impl Pending {
    fn settle(mut self) {
        self.0 = None;
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        let Some(link) = self.0.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        drop(runtime.spawn(async move {
            if let Some(link) = link.upgrade() {
                link.cancel_inflight().await;
            }
        }));
    }
}

impl Link {
    pub(super) async fn cancel_inflight(&self) {
        if let Some(conn) = self.conn.lock().await.as_mut() {
            conn.cancel_inflight().await;
        }
    }
}
