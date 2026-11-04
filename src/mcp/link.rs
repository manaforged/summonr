use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, Weak};

use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::policy::{Action, Policy};
use crate::rpc::Notice;
use crate::supervise::{self, ServerKind, Slot};
use summonr_core::error::{Error, Result};

use super::catalog::{self, Listing};
use super::config::Server;
use super::conn::Conn;
use super::status::McpState;
use super::tool::{self, Tool};
use super::{Answer, McpProgress, McpUpdate};
use summonr_types::Form;

pub(super) struct Link {
    server: Server,
    workspace: PathBuf,
    conn: Slot<Conn>,
    fresh: std::sync::Mutex<Option<HashSet<String>>>,
    phase: std::sync::Mutex<Phase>,
    refresh: Option<tokio::task::JoinHandle<()>>,
    pub(super) tools: Vec<Tool>,
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(refresh) = &self.refresh {
            refresh.abort();
        }
    }
}

#[derive(Clone)]
enum Phase {
    Cached,
    Connecting,
    Live,
    Stopped(Option<String>),
    Failed(String),
}

impl Link {
    pub(super) async fn open(server: Server, workspace: PathBuf) -> Result<Arc<Self>> {
        let slot: Slot<Conn> = Arc::new(Mutex::new(None));
        let (listing, phase, refresh) = match catalog::load(&server.key) {
            Some(listing) => {
                let (server, workspace, slot) = (server.clone(), workspace.clone(), slot.clone());
                let refresh = tokio::spawn(async move {
                    drop(Self::list(&server, &workspace, &slot).await);
                });
                (listing, Phase::Cached, Some(refresh))
            }
            None => {
                let listing = Self::list(&server, &workspace, &slot).await?;
                let phase = match slot.lock().await.as_ref() {
                    Some(_) => Phase::Live,
                    None => Phase::Cached,
                };
                (listing, phase, None)
            }
        };
        let tools = tool::offered(&server, &listing.tools)
            .map(|(_, tool)| tool)
            .collect();
        Ok(Arc::new(Self {
            server,
            workspace,
            conn: slot,
            fresh: std::sync::Mutex::new(None),
            phase: std::sync::Mutex::new(phase),
            refresh,
            tools,
        }))
    }

    pub(super) async fn list(
        server: &Server,
        workspace: &std::path::Path,
        slot: &Slot<Conn>,
    ) -> Result<Listing> {
        let mut held = slot.lock().await;
        let listing = Self::first(server, workspace, slot, &mut held)
            .await
            .inspect_err(|error| catalog::fail(&server.key, &error.to_string()))?;
        catalog::save(&server.key, &listing);
        if let Some(conn) = held.as_mut() {
            conn.ready(server.wait);
        }
        if server.idle.is_some() {
            *held = None;
        }
        Ok(listing)
    }

    async fn first(
        server: &Server,
        workspace: &std::path::Path,
        slot: &Slot<Conn>,
        held: &mut Option<Conn>,
    ) -> Result<Listing> {
        let conn = held.insert(Conn::open(server, workspace).await?);
        supervise::register(ServerKind::Mcp, &server.name, conn.pid(), server.idle, slot);
        conn.listing().await
    }
    pub(super) async fn call_tool(
        self: &Arc<Self>,
        tool: &Tool,
        arguments: Value,
        gate: &Gate<'_>,
        report: &mut (dyn FnMut(McpUpdate) + Send),
    ) -> Result<Called> {
        gate.policy
            .decide(Action::Tool {
                alias: &tool.alias,
                read_only: false,
            })
            .into_result()?;
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
        self.set(Phase::Connecting);
        let connecting = Connecting(Some(self.as_ref()));
        let opened = Conn::open(&self.server, &self.workspace).await;
        connecting.settle();
        let mut conn = opened.inspect_err(|error| {
            catalog::fail(&self.server.key, &error.to_string());
            self.set(Phase::Failed(error.to_string()));
        })?;
        self.set(Phase::Live);
        if let Ok(listing) = conn.listing().await {
            catalog::save(&self.server.key, &listing);
            self.remember(&listing.tools);
        }
        conn.ready(self.server.wait);
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
            Notice::Message { .. } | Notice::Unknown => {}
        }
    }

    async fn after(&self, slot: &mut Option<Conn>, failed: Option<&Error>, changes: Changes) {
        let Some(conn) = slot.as_mut() else {
            return;
        };
        if let Some(error) = failed
            && !conn.alive()
        {
            catalog::fail(&self.server.key, &error.to_string());
            self.set(Phase::Stopped(Some(error.to_string())));
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
        let phase = self
            .phase
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match phase {
            Phase::Cached => (McpState::Cached, None),
            Phase::Connecting => (McpState::Connecting, None),
            Phase::Live if self.conn.try_lock().is_ok_and(|slot| slot.is_none()) => {
                (McpState::Stopped, None)
            }
            Phase::Live => (McpState::Live, None),
            Phase::Stopped(error) => (McpState::Stopped, error),
            Phase::Failed(error) => (McpState::Failed, Some(error)),
        }
    }

    fn set(&self, next: Phase) {
        let mut phase = self.phase.lock().unwrap_or_else(PoisonError::into_inner);
        *phase = next;
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

struct Connecting<'a>(Option<&'a Link>);

impl Connecting<'_> {
    fn settle(mut self) {
        self.0 = None;
    }
}

impl Drop for Connecting<'_> {
    fn drop(&mut self) {
        if let Some(link) = self.0.take() {
            link.set(Phase::Stopped(None));
        }
    }
}

impl Link {
    pub(super) async fn cancel_inflight(&self) {
        if let Some(conn) = self.conn.lock().await.as_mut() {
            conn.cancel_inflight().await;
        }
    }
}
