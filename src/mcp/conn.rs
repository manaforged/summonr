use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::catalog::Listing;
use super::config::{Launch, Server};
use super::http;
use crate::error::{Error, Result};
use crate::limits::MCP_OPEN;
use crate::rpc::{self, Client, Notice};
use crate::tools::sandbox::Sandbox;

const STATELESS: &str = "2026-07-28";
const LIST_PAGES: usize = 64;

pub(super) struct Conn {
    transport: Transport,
    stateless: bool,
    sandbox: Option<Sandbox>,
    offers: Value,
}

enum Transport {
    Stdio(Box<Client>),
    Http(http::Http),
}

impl Conn {
    pub(super) async fn open(
        server: &Server,
        workspace: &Path,
        sandbox: Option<&Sandbox>,
    ) -> Result<Self> {
        let transport = match &server.launch {
            Launch::Stdio { command, args, env } => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                Transport::Stdio(Box::new(Client::spawn(&rpc::Spawn {
                    program: command,
                    args: &args,
                    env,
                    cwd: workspace,
                    sandbox,
                    wait: MCP_OPEN.min(server.wait),
                })?))
            }
            Launch::Http { url, headers, auth } => Transport::Http(http::Http::new(
                url,
                headers,
                MCP_OPEN.min(server.wait),
                auth.clone()
                    .map(|spec| {
                        crate::credentials::mcp_auth(*spec, &format!("mcp-{}", server.name))
                    })
                    .transpose()?,
            )?),
        };
        let mut conn = Self {
            transport,
            stateless: false,
            sandbox: sandbox.cloned(),
            offers: json!({"tools": {}}),
        };
        let agreed = match &mut conn.transport {
            Transport::Stdio(client) => Some(client.initialize("summonr").await?),
            Transport::Http(http) => http.initialize(rpc::hello("summonr", json!({}))).await?,
        };
        conn.stateless = agreed.is_none();
        if let Some(offers) =
            agreed.and_then(|mut result| result.get_mut("capabilities").map(Value::take))
        {
            conn.offers = offers;
        }
        Ok(conn)
    }

    pub(super) fn ready(&mut self, wait: Duration) -> Result<()> {
        match &mut self.transport {
            Transport::Stdio(client) => {
                client.set_wait(wait);
                Ok(())
            }
            Transport::Http(http) => http.set_wait(wait),
        }
    }

    pub(super) fn runs_in(&self, sandbox: Option<&Sandbox>) -> bool {
        match &self.transport {
            Transport::Stdio(_) => self.sandbox.as_ref() == sandbox,
            Transport::Http(_) => true,
        }
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

    pub(super) fn alive(&mut self) -> bool {
        match &mut self.transport {
            Transport::Stdio(client) => client.alive(),
            Transport::Http(http) => !http.expired(),
        }
    }

    pub(super) fn cancel_inflight(&mut self) {
        if let Transport::Stdio(client) = &mut self.transport {
            client.cancel_inflight();
        }
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
        match &mut self.transport {
            Transport::Stdio(client) => client.call(method, params, on_note).await,
            Transport::Http(http) => http.call(method, params, on_note).await,
        }
    }
}
