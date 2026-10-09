use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::policy::Policy;
use crate::tools::ToolOutput;
use crate::tools::sandbox::Sandbox;

mod catalog;
mod config;
mod conn;
mod defer;
mod expand;
mod form;
mod http;
mod journal;
mod link;
mod manage;
mod offer;
mod output;
mod probe;
mod status;
mod tool;

pub(crate) use config::Entry;
pub use defer::call_label;
pub(crate) use defer::{Resolved, SEARCH_TOOL, effective};
pub(crate) use expand::wanted;
pub use form::{Choice, Field, FieldKind, Form, FormAction, FormAnswer, TextFormat};
use link::{Gate, Link};
pub use manage::{McpLaunch, add, remove};
pub(crate) use offer::{Offer, OfferKind, offers};
use output::output;
pub use probe::probe;
pub use status::{Board, McpState, McpTransport, ServerStatus, status};
use tool::Tool;

pub(crate) enum McpUpdate {
    Progress(McpProgress),
    Elicit { form: Form, answer: Answer },
}

pub(crate) struct Answer(tokio::sync::oneshot::Sender<crate::rpc::Reply>);

impl Answer {
    pub(crate) fn give(self, answer: &FormAnswer) {
        drop(self.0.send(crate::rpc::Reply::Result(answer.wire())));
    }
}

pub(crate) struct McpProgress {
    pub(crate) progress: f64,
    pub(crate) total: Option<f64>,
    pub(crate) message: Option<String>,
}

pub struct Hub {
    links: Vec<Arc<Link>>,
    failures: Vec<(String, String)>,
    pub(crate) notes: Vec<String>,
}

impl Hub {
    pub(crate) async fn connect(workspace: PathBuf, sandbox: Option<Sandbox>) -> Self {
        let (found, failed) = match config::servers(&workspace) {
            Ok(found) => (found, None),
            Err(error) => (
                config::Found {
                    servers: Vec::new(),
                    waiting: Vec::new(),
                },
                Some(format!("mcp config: {error}")),
            ),
        };
        let opened = futures::future::join_all(found.servers.into_iter().map(|server| {
            let workspace = workspace.clone();
            let sandbox = sandbox.as_ref();
            async move {
                let name = server.name.clone();
                Link::open(server, workspace, sandbox)
                    .await
                    .map_err(|error| (name, error.to_string()))
            }
        }))
        .await;
        let mut hub = Self {
            links: Vec::new(),
            failures: Vec::new(),
            notes: Vec::new(),
        };
        for result in opened {
            match result {
                Ok(link) => hub.links.push(link),
                Err((name, error)) => {
                    hub.notes.push(format!("mcp {name}: {error}"));
                    hub.failures.push((name, error));
                }
            }
        }
        hub.notes.extend(failed);
        if !found.waiting.is_empty() {
            hub.notes.push(format!(
                "project MCP servers wait for approval: {}. Set `enabled = true` under [mcp.NAME] in ~/.summonr/config.toml to run one",
                found.waiting.join(", ")
            ));
        }
        hub
    }

    pub(crate) fn state(&self, name: &str) -> Option<(McpState, Option<String>)> {
        if let Some(link) = self.links.iter().find(|link| link.name() == name) {
            return Some(link.state());
        }
        self.failures
            .iter()
            .find(|(failed, _)| failed == name)
            .map(|(_, error)| (McpState::Failed, Some(error.clone())))
    }

    pub(crate) async fn cancel_inflight(&self) {
        for link in &self.links {
            link.cancel_inflight().await;
        }
    }

    pub(crate) fn owns(&self, name: &str) -> bool {
        name == SEARCH_TOOL || self.find(name).is_some()
    }

    pub(crate) async fn call(
        &self,
        alias: &str,
        arguments: Value,
        policy: &Policy,
        sandbox: Option<&Sandbox>,
        report: &mut (dyn FnMut(McpUpdate) + Send),
    ) -> Result<ToolOutput> {
        let (link, item) = self
            .find(alias)
            .ok_or_else(|| Error::Tool(format!("unknown tool `{alias}`")))?;
        let gate = Gate { policy, sandbox };
        let called = link.call_tool(item, arguments, &gate, report).await?;
        let mut shown = output(called.value, !called.readonly);
        if called.changed {
            shown.text.push_str(&format!(
                "\n[mcp {} changed its tool list; the new list applies from the next session]",
                link.name()
            ));
        }
        Ok(shown)
    }

    fn find(&self, alias: &str) -> Option<(&Arc<Link>, &Tool)> {
        self.links.iter().find_map(|link| {
            link.tools
                .iter()
                .find(|item| item.alias == alias)
                .map(|item| (link, item))
        })
    }
}
