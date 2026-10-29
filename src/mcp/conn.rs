use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::catalog::Listing;
use super::config::{Launch, Server};
use super::handshake;
use super::http::Http;
use super::journal::Journal;
use crate::process::Env;
use crate::rpc::{self, Client, Framing, Notice, Peer};
use summonr_core::error::{Error, Result};

const STATELESS: &str = "2026-07-28";
const LIST_PAGES: usize = 64;

pub(super) struct Conn {
    client: Box<Client>,
    stateless: bool,
    offers: Value,
    server: String,
    journal: Journal,
}

fn logged<'n>(journal: &Journal, server: &str, notice: Notice<'n>) -> Option<Notice<'n>> {
    match notice {
        Notice::Message {
            level,
            logger,
            data,
        } => {
            journal.append(server, level, logger, data);
            None
        }
        other => Some(other),
    }
}

impl Conn {
    pub(super) async fn open(server: &Server, workspace: &Path) -> Result<Self> {
        let wait = crate::limits::limits().mcp_open().min(server.wait);
        let client = match &server.launch {
            Launch::Stdio { command, args, env } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                Client::spawn(&rpc::Spawn {
                    program: command,
                    args: &args,
                    env,
                    cwd: workspace,
                    wait,
                    base: Env::Clean,
                    framing: Framing::Line,
                    max: crate::limits::limits().mcp_frame_max_bytes,
                    peer: Peer::Mcp,
                })?
            }
            Launch::Http { url, headers, auth } => Client::attach(
                Box::new(Http::new(
                    url,
                    headers,
                    server.wait,
                    auth.clone()
                        .map(|spec| {
                            crate::credentials::mcp_auth(*spec, &format!("mcp-{}", server.name))
                        })
                        .transpose()?,
                )?),
                workspace,
                wait,
            ),
        };
        let mut conn = Self {
            client: Box::new(client),
            stateless: false,
            offers: json!({"tools": {}}),
            server: server.name.clone(),
            journal: Journal::default(),
        };
        let (journal, name) = (&conn.journal, conn.server.as_str());
        let mut hear = |notice: Notice<'_>| drop(logged(journal, name, notice));
        let agreed = handshake::initialize(&mut conn.client, "summonr", &mut hear).await?;
        conn.stateless = agreed.is_none();
        if let Some(offers) =
            agreed.and_then(|mut result| result.get_mut("capabilities").map(Value::take))
        {
            conn.offers = offers;
        }
        Ok(conn)
    }

    pub(super) fn ready(&mut self, wait: Duration) {
        self.client.set_wait(wait);
    }

    pub(super) async fn listing(&mut self) -> Result<Listing> {
        let mut listing = Listing::default();
        if self.offers.get("tools").is_some() {
            listing.tools = self.pages("tools/list", "tools").await?;
        }
        if self.offers.get("prompts").is_some() {
            listing.prompts = self
                .pages("prompts/list", "prompts")
                .await
                .unwrap_or_default();
        }
        if self.offers.get("resources").is_some() {
            listing.resources = self
                .pages("resources/list", "resources")
                .await
                .unwrap_or_default();
            listing.templates = self
                .pages("resources/templates/list", "resourceTemplates")
                .await
                .unwrap_or_default();
        }
        Ok(listing)
    }

    async fn pages(&mut self, method: &str, key: &str) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..LIST_PAGES {
            let params = cursor.map_or_else(|| json!({}), |cursor| json!({"cursor": cursor}));
            let mut page = self.call(method, params, &mut |_| {}).await?;
            if let Some(Value::Array(found)) = page.get_mut(key).map(Value::take) {
                items.extend(found);
            }
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            if cursor.is_none() {
                return Ok(items);
            }
        }
        Err(Error::Tool(format!(
            "`{method}` has more than {LIST_PAGES} pages"
        )))
    }

    pub(super) fn pid(&self) -> Option<u32> {
        self.client.pid()
    }

    pub(super) fn alive(&mut self) -> bool {
        self.client.alive()
    }

    pub(super) async fn cancel_inflight(&mut self) {
        self.client.cancel_inflight().await;
    }

    pub(super) async fn call(
        &mut self,
        method: &str,
        mut params: Value,
        on_note: &mut (dyn FnMut(Notice<'_>) + Send),
    ) -> Result<Value> {
        if self.stateless
            && let Some(object) = params.as_object_mut()
        {
            object.insert(
                "_meta".to_string(),
                json!({
                    "io.modelcontextprotocol/protocolVersion": STATELESS,
                    "io.modelcontextprotocol/clientInfo": {"name": "summonr", "version": env!("CARGO_PKG_VERSION")}
                }),
            );
        }
        let (journal, name) = (&self.journal, self.server.as_str());
        let mut hear = |notice: Notice<'_>| {
            if let Some(other) = logged(journal, name, notice) {
                on_note(other);
            }
        };
        self.client.call(method, params, &mut hear).await
    }
}
